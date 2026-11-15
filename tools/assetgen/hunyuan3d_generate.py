"""Hunyuan3D-2 (Tencent) image -> 3D asset (GLB) without ComfyUI.

Runs the Hunyuan3D-2 code vendored by ComfyUI-Hunyuan3DWrapper (read-only, imported in place)
with local weights:
  shape  : hunyuan3d-dit-v2-0-turbo (+ hunyuan3d-vae-v2-0-turbo FlashVDM decoder when present)
  texture: hunyuan3d-delight-v2-0 + hunyuan3d-paint-v2-0-turbo, rasterised through the nvdiffrast
           backed ``custom_rasterizer`` shim in ./shims (the original CUDA extension is cp312-only)

Usage (embedded python ignores PYTHONPATH; paths are set up by this script):
  python.exe -s hunyuan3d_generate.py --image in.png --out out.glb [--texture] [--faces 15000]
            [--seed 1234] [--steps 5] [--octree 256] [--models E:\\ComfyUI\\hy3dgen_models]
  python.exe -s hunyuan3d_generate.py --batch jobs.json --out out_dir [--texture]
  python.exe -s hunyuan3d_generate.py --check            (imports + weights only, no inference)

Batch file: a JSON list (or {"defaults": {...}, "jobs": [...]}) of objects with "image" (required,
relative to the JSON file) and optional "out"/"name", "texture", "faces", "seed", "steps", "octree",
"guidance", "height".  Unknown keys (e.g. a text "prompt") are ignored.  Models are loaded once.
"""

import argparse
import gc
import json
import os
import shutil
import sys
import time
import traceback
import types

HERE = os.path.dirname(os.path.abspath(__file__))
SHIM_DIR = os.path.join(HERE, "shims")
DEFAULT_WRAPPER_DIR = r"E:\ComfyUI\ComfyUIStudio\App\ComfyUI\custom_nodes\ComfyUI-Hunyuan3DWrapper"
DEFAULT_MODELS = r"E:\ComfyUI\hy3dgen_models"
REPO = os.path.join("tencent", "Hunyuan3D-2")

# --- sys.path: shims first (they must shadow ComfyUI's comfy/ + folder_paths that python311._pth exposes) ---
if SHIM_DIR not in sys.path:
    sys.path.insert(0, SHIM_DIR)


def log(msg):
    print(f"[hy3d] {msg}", flush=True)


def parse_args(argv=None):
    p = argparse.ArgumentParser(description="Hunyuan3D-2 turbo image->3D (shape + optional texture), no ComfyUI")
    io = p.add_argument_group("input/output")
    io.add_argument("--image", help="input image (PNG/JPG). Background is removed with rembg if it has no alpha")
    io.add_argument("--out", help="output .glb (single mode) or output directory (batch mode)")
    io.add_argument("--batch", help="JSON job list, processed sequentially with the models loaded once")
    io.add_argument("--check", action="store_true", help="only verify imports, shim and weights; no inference")
    io.add_argument("--debug-dir", help="save intermediates (rgba input, delit image, views, maps) here")
    io.add_argument("--height", type=float, default=None,
                    help="scale the final mesh to this height (scene units/metres) and put its base at y=0")

    sh = p.add_argument_group("shape")
    sh.add_argument("--faces", type=int, default=15000, help="target face count after decimation")
    sh.add_argument("--seed", type=int, default=1234)
    sh.add_argument("--steps", type=int, default=5, help="DiT sampling steps (turbo: 5)")
    sh.add_argument("--guidance", type=float, default=5.0, help="guidance (embedded guidance for the turbo DiT)")
    sh.add_argument("--octree", type=int, default=256, help="octree / marching-cubes resolution")
    sh.add_argument("--num-chunks", type=int, default=20000)
    sh.add_argument("--box-v", type=float, default=1.01)
    sh.add_argument("--mc-algo", default="mc", choices=["mc", "dmc"])
    sh.add_argument("--no-flashvdm", action="store_true", help="dense volume decoding instead of FlashVDM")
    sh.add_argument("--no-postprocess", action="store_true", help="skip floater/degenerate removal + decimation")
    sh.add_argument("--keep-background", action="store_true", help="never run background removal")
    sh.add_argument("--rembg-model", default="u2net", help="rembg session model (upstream: u2net)")

    tx = p.add_argument_group("texture")
    tx.add_argument("--texture", action="store_true", help="also run delight + multiview paint + bake")
    tx.add_argument("--render-size", type=int, default=1024, help="bake render resolution (upstream 2048)")
    tx.add_argument("--texture-size", type=int, default=1024, help="UV texture resolution (upstream 2048)")
    tx.add_argument("--paint-steps", type=int, default=None,
                    help="multiview steps (turbo default: upstream's fixed 10-step LCM schedule; non-turbo: 30)")
    tx.add_argument("--paint-guidance", type=float, default=None, help="non-turbo CFG (default 2.0)")
    tx.add_argument("--delight-steps", type=int, default=50)
    tx.add_argument("--no-delight", action="store_true")
    tx.add_argument("--low-vram", action="store_true",
                    help="diffusers model CPU offload for delight/paint (slower, lowest VRAM)")

    md = p.add_argument_group("models")
    md.add_argument("--models", default=DEFAULT_MODELS, help="folder that contains tencent/Hunyuan3D-2/...")
    md.add_argument("--wrapper-dir", default=DEFAULT_WRAPPER_DIR, help="ComfyUI-Hunyuan3DWrapper folder (read only)")
    md.add_argument("--dit", default="hunyuan3d-dit-v2-0-turbo")
    md.add_argument("--vae", default="hunyuan3d-vae-v2-0-turbo", help="FlashVDM VAE subfolder ('' = bundled VAE)")
    md.add_argument("--paint", default="hunyuan3d-paint-v2-0-turbo")
    md.add_argument("--delight", default="hunyuan3d-delight-v2-0")
    md.add_argument("--raster-backend", default=None, choices=["nvdiffrast", "torch"],
                    help="custom_rasterizer shim backend (default nvdiffrast)")
    args = p.parse_args(argv)
    if not args.check and not args.image and not args.batch:
        p.error("one of --image, --batch or --check is required")
    if args.image and not args.out:
        p.error("--out is required with --image")
    return args


def setup_environment(args):
    """Env + sys.path before anything imports the vendored code."""
    tmp = os.path.join(HERE, ".tmp", f"run_{os.getpid()}")
    os.makedirs(tmp, exist_ok=True)
    os.environ.setdefault("HY3D_TEMP_DIR", tmp)  # shims/folder_paths.py temp_directory (pymeshlab round trips)
    os.environ.setdefault("U2NET_HOME", os.path.join(HERE, "models", "u2net"))  # rembg weights stay in the project
    os.environ.setdefault("NUMBA_CACHE_DIR", os.path.join(HERE, ".numba_cache"))
    if args.raster_backend:
        os.environ["CUSTOM_RASTERIZER_BACKEND"] = args.raster_backend
    import tempfile
    tempfile.tempdir = tmp  # DegenerateFaceRemover uses NamedTemporaryFile
    try:
        sys.stdout.reconfigure(errors="replace")
        sys.stderr.reconfigure(errors="replace")
    except Exception:
        pass
    import hy3d_bootstrap
    pkg = hy3d_bootstrap.bootstrap(args.wrapper_dir, offline=True)  # shims at sys.path[0], wrapper dir appended
    return pkg, tmp


# ----------------------------------------------------------------------------------------------------------------
# helpers
# ----------------------------------------------------------------------------------------------------------------

class Stats:
    def __init__(self):
        self.timings = []
        self.peaks = {}

    def stage(self, name):
        stats = self

        class _Stage:
            def __enter__(self_):
                import torch
                if torch.cuda.is_available():
                    torch.cuda.synchronize()
                    torch.cuda.reset_peak_memory_stats()
                self_.t0 = time.perf_counter()
                return self_

            def __exit__(self_, exc_type, exc, tb):
                import torch
                peak = 0.0
                if torch.cuda.is_available():
                    torch.cuda.synchronize()
                    peak = torch.cuda.max_memory_allocated() / 2 ** 30
                dt = time.perf_counter() - self_.t0
                stats.timings.append((name, dt))
                stats.peaks[name] = max(stats.peaks.get(name, 0.0), peak)
                log(f"{name}: {dt:.2f}s, peak VRAM {peak:.2f} GB" + (" (failed)" if exc_type else ""))
                return False

        return _Stage()

    def report(self, title):
        total = sum(t for _, t in self.timings)
        log(f"---- {title}: total {total:.1f}s")
        for name, t in self.timings:
            log(f"   {name:<28s} {t:8.2f}s   peak {self.peaks.get(name, 0):5.2f} GB")
        if self.peaks:
            log(f"   max peak VRAM (torch allocated): {max(self.peaks.values()):.2f} GB")


def free_vram():
    import torch
    gc.collect()
    if torch.cuda.is_available():
        torch.cuda.empty_cache()


def has_real_alpha(img):
    import numpy as np
    if img.mode not in ("RGBA", "LA") and not (img.mode == "P" and "transparency" in img.info):
        return False
    alpha = np.asarray(img.convert("RGBA"))[..., 3]
    return bool((alpha < 250).any())


def require(path, what):
    if not os.path.exists(path):
        raise FileNotFoundError(f"{what} not found: {path}")
    return path


# ----------------------------------------------------------------------------------------------------------------
# shape
# ----------------------------------------------------------------------------------------------------------------

class ShapeGenerator:
    def __init__(self, args, repo_dir, device, stats):
        import torch
        import yaml
        from hy3d_bootstrap import hy3d_import

        self.args, self.device = args, device
        self.cpu = torch.device("cpu")
        pipes = hy3d_import("hy3dgen.shapegen.pipelines")
        schedulers = hy3d_import("hy3dgen.shapegen.schedulers")
        self.post = hy3d_import("hy3dgen.shapegen.postprocessors")
        ShapeVAE = hy3d_import("hy3dgen.shapegen.models.autoencoders").ShapeVAE

        dit_dir = require(os.path.join(repo_dir, args.dit), "DiT folder")
        dit_ckpt = require(os.path.join(dit_dir, "model.fp16.safetensors"), "DiT weights")
        with open(os.path.join(dit_dir, "config.yaml"), "r", encoding="utf-8") as fh:
            dit_cfg = yaml.safe_load(fh)
        sched_cfg = dit_cfg.get("scheduler", {})
        consistency = "Consistency" in str(sched_cfg.get("target", ""))

        with stats.stage("load shape models"):
            # Exact loader of the vendored wrapper (nodes.py Hy3DModelLoader): combined fp16 checkpoint with
            # model./vae./conditioner. prefixes; config = wrapper configs/dit_config.yaml, guidance_embed
            # auto-detected from 'guidance_in.in_layer.bias'.  Everything is materialised on the CPU.
            self.pipe, bundled_vae = pipes.Hunyuan3DDiTFlowMatchingPipeline.from_single_file(
                ckpt_path=dit_ckpt, device=device, offload_device=self.cpu, dtype=torch.float16,
                use_safetensors=True, attention_mode="sdpa",
                scheduler="ConsistencyFlowMatchEulerDiscreteScheduler" if consistency
                else "FlowMatchEulerDiscreteScheduler")
            params = sched_cfg.get("params", {})
            if consistency:
                self.pipe.scheduler = schedulers.ConsistencyFlowMatchEulerDiscreteScheduler(
                    num_train_timesteps=params.get("num_train_timesteps", 1000),
                    pcm_timesteps=params.get("pcm_timesteps", 100))
            # The wrapper's ImageEncoder skips resize+ImageNet-normalisation for guidance-distilled checkpoints
            # (has_guidance_embed) unless the input is already 518/530 px; upstream Tencent code always applies
            # Resize(518)+CenterCrop+Normalize.  Restore upstream behaviour.
            self.pipe.conditioner.main_image_encoder.has_guidance_embed = False

            vae_dir = os.path.join(repo_dir, args.vae) if args.vae else ""
            vae_ckpt = os.path.join(vae_dir, "model.fp16.safetensors") if vae_dir else ""
            if vae_ckpt and os.path.exists(vae_ckpt) and os.path.exists(os.path.join(vae_dir, "config.yaml")):
                # upstream enable_flashvdm(replace_vae=True): ShapeVAE.from_pretrained(..., 'hunyuan3d-vae-v2-0-turbo')
                # -> VectsetVAE.from_single_file(ckpt, config.yaml) (local path, no smart_load_model/HF download)
                self.vae = ShapeVAE.from_single_file(vae_ckpt, os.path.join(vae_dir, "config.yaml"),
                                                     device="cpu", dtype=torch.float16, use_safetensors=True)
                self.vae_name = args.vae
                del bundled_vae
            else:
                self.vae = bundled_vae
                self.vae_name = "bundled (from DiT checkpoint)"
            self.vae.eval()
        log(f"shape: DiT={args.dit} (guidance_embed={self.pipe.model.guidance_embed}, "
            f"scheduler={type(self.pipe.scheduler).__name__}), VAE={self.vae_name}")

        # Keep the DiT on the CPU while the 1.1B DINOv2 conditioner runs (the vendored encode_cond moves the
        # conditioner GPU->CPU itself); move the DiT up only afterwards -> lower peak VRAM.
        base_encode = pipes.Hunyuan3DDiTPipeline.encode_cond

        def encode_cond(pipe_self, *a, **kw):
            out = base_encode(pipe_self, *a, **kw)
            free_vram()
            pipe_self.model.to(pipe_self.main_device)
            return out

        self.pipe.encode_cond = types.MethodType(encode_cond, self.pipe)
        self._rembg = None

    def remove_background(self, img):
        if self._rembg is None:
            from rembg import new_session
            os.makedirs(os.environ["U2NET_HOME"], exist_ok=True)
            # CPU provider: u2net at 320 px is ~1 s on CPU, while onnxruntime-gpu's TensorRT/CUDA provider probing
            # takes tens of seconds (and logs errors) in this environment
            self._rembg = new_session(self.args.rembg_model, providers=["CPUExecutionProvider"])
        from rembg import remove
        return remove(img, session=self._rembg, bgcolor=[255, 255, 255, 0])  # = hy3dgen/rembg.py

    @staticmethod
    def _flip_faces(outputs):
        outputs.mesh_f = outputs.mesh_f[:, ::-1]
        return outputs

    def generate(self, img_rgba, job, stats):
        import torch
        import trimesh
        a = self.args
        with stats.stage("shape: DiT sampling"):
            prep = self.pipe.image_processor(img_rgba)  # ImageProcessorV2(512, border 0.15): recentre on white
            image = prep["image"].to(self.device, torch.float16)
            try:
                latents = self.pipe(image=image, mask=None, num_inference_steps=job["steps"],
                                    guidance_scale=job["guidance"], generator=torch.manual_seed(job["seed"]))
            finally:
                self.pipe.model.to(self.cpu)
                self.pipe.conditioner.to(self.cpu)
                free_vram()
        with stats.stage("shape: VAE decode + MC"):
            try:
                vae = self.vae.to(self.device)
                vae.enable_flashvdm_decoder(enabled=not a.no_flashvdm, mc_algo=a.mc_algo)
                with torch.no_grad():
                    lat = vae(1.0 / vae.scale_factor * latents)
                    out = vae.latents2mesh(lat, bounds=a.box_v, mc_level=0.0, num_chunks=a.num_chunks,
                                           octree_resolution=job["octree"], mc_algo=a.mc_algo, enable_pbar=False)[0]
                del lat, latents
            finally:
                self.vae.to(self.cpu)
                free_vram()
            if out is None:
                raise RuntimeError("surface extraction produced no mesh (empty volume?)")
            out = self._flip_faces(out)  # export_to_trimesh / Hy3DVAEDecode
            mesh = trimesh.Trimesh(out.mesh_v, out.mesh_f)
        log(f"raw mesh: {len(mesh.vertices)} verts, {len(mesh.faces)} faces")
        if not a.no_postprocess:
            with stats.stage("shape: postprocess"):
                mesh = self.post.FloaterRemover()(mesh)
                mesh = self.post.DegenerateFaceRemover()(mesh)
                mesh = self.post.FaceReducer()(mesh, max_facenum=job["faces"])
            log(f"postprocessed mesh: {len(mesh.vertices)} verts, {len(mesh.faces)} faces")
        return mesh


# ----------------------------------------------------------------------------------------------------------------
# texture
# ----------------------------------------------------------------------------------------------------------------

def recorrect_rgb(src_image, target_image, alpha_channel, scale=0.95):
    """Upstream Light_Shadow_Remover.recorrect_rgb (newer than the vendored copy)."""
    import torch

    def flat_and_mask(bgr, a):
        mask = torch.where(a > 0.5, True, False)
        return bgr.reshape(-1, bgr.shape[-1])[mask.reshape(-1), :]

    src_flat = flat_and_mask(src_image, alpha_channel)
    target_flat = flat_and_mask(target_image, alpha_channel)
    corrected = torch.zeros_like(src_image)
    for i in range(3):
        src_mean, src_std = torch.mean(src_flat[:, i]), torch.std(src_flat[:, i])
        tgt_mean, tgt_std = torch.mean(target_flat[:, i]), torch.std(target_flat[:, i])
        corrected[:, :, i] = torch.clamp((src_image[:, :, i] - scale * src_mean) * (tgt_std / src_std)
                                         + scale * tgt_mean, 0, 1)
    src_mse = torch.mean((src_image - target_image) ** 2)
    modify_mse = torch.mean((corrected - target_image) ** 2)
    out = src_image if src_mse < modify_mse else corrected
    return torch.cat([out, alpha_channel], dim=-1)


def recenter_image(image, border_ratio=0.2):
    """Upstream Hunyuan3DPaintPipeline.recenter_image."""
    import numpy as np
    from PIL import Image
    if image.mode == "RGB":
        return image
    if image.mode == "L":
        return image.convert("RGB")
    alpha = np.array(image)[:, :, 3]
    nz = np.argwhere(alpha > 0)
    if nz.size == 0:
        raise ValueError("Image is fully transparent")
    min_row, min_col = nz.min(axis=0)
    max_row, max_col = nz.max(axis=0)
    cropped = image.crop((min_col, min_row, max_col + 1, max_row + 1))
    width, height = cropped.size
    bw, bh = int(width * border_ratio), int(height * border_ratio)
    new_w, new_h = width + 2 * bw, height + 2 * bh
    square = max(new_w, new_h)
    canvas = Image.new("RGBA", (square, square), (255, 255, 255, 0))
    canvas.paste(cropped, ((square - new_w) // 2 + bw, (square - new_h) // 2 + bh))
    return canvas


class TextureGenerator:
    CAMERA_AZIMS = [0, 90, 180, 270, 0, 180]
    CAMERA_ELEVS = [0, 0, 0, 0, 90, -90]
    VIEW_WEIGHTS = [1, 0.1, 0.5, 0.1, 0.05, 0.05]
    BAKE_EXP = 4
    # upstream turbo: DDIMSolver(ddim_timesteps=30).ddim_timesteps[torch.range(29, 0, -3)]
    TURBO_TIMESTEPS = [989, 890, 791, 692, 593, 494, 395, 296, 197, 98]

    def __init__(self, args, repo_dir, device, stats):
        import json as _json

        import safetensors.torch
        import torch
        from accelerate import init_empty_weights
        from accelerate.utils import set_module_tensor_to_device
        from diffusers import (AutoencoderKL, DDIMScheduler, EulerAncestralDiscreteScheduler, LCMScheduler,
                               StableDiffusionInstructPix2PixPipeline)
        from transformers import CLIPImageProcessor, CLIPTokenizer

        import mesh_processor_fast
        from hy3d_bootstrap import hy3d_import

        self.args, self.device = args, device
        mr = hy3d_import("hy3dgen.texgen.differentiable_renderer.mesh_render")
        mesh_processor_fast.install(mr)  # numba port of mesh_processor.cpp instead of the slow .py fallback
        self.MeshRender = mr.MeshRender
        unet_mod = hy3d_import("hy3dgen.texgen.hunyuanpaint.unet.modules")
        HunyuanPaintPipeline = hy3d_import("hy3dgen.texgen.hunyuanpaint.pipeline").HunyuanPaintPipeline
        import custom_rasterizer
        log(f"custom_rasterizer shim: {custom_rasterizer.__file__} (backend={custom_rasterizer.get_backend()})")

        self.turbo = "turbo" in args.paint
        with stats.stage("load texture models"):
            self.delight = None
            if not args.no_delight:
                # = vendored Light_Shadow_Remover.__init__ (kept on CPU; moved to GPU only while used)
                ddir = require(os.path.join(repo_dir, args.delight), "delight model")
                self.delight = StableDiffusionInstructPix2PixPipeline.from_pretrained(
                    ddir, torch_dtype=torch.float16, safety_checker=None)
                self.delight.scheduler = EulerAncestralDiscreteScheduler.from_config(self.delight.scheduler.config)
                self.delight.set_progress_bar_config(disable=True)

            # = wrapper nodes.py DownloadAndLoadHy3DPaintModel.loadmodel (UNet2p5D built from unet/config.json,
            # weights from unet/diffusion_pytorch_model.safetensors, other parts via diffusers/transformers)
            pdir = require(os.path.join(repo_dir, args.paint), "paint model")
            with open(os.path.join(pdir, "unet", "config.json"), "r", encoding="utf-8") as fh:
                ucfg = _json.load(fh)
            with init_empty_weights():
                unet = unet_mod.UNet2DConditionModel(**ucfg)
                unet = unet_mod.UNet2p5DConditionModel(unet)
            st = os.path.join(pdir, "unet", "diffusion_pytorch_model.safetensors")
            if os.path.exists(st):
                unet_sd = safetensors.torch.load_file(st)
            else:
                unet_sd = torch.load(require(os.path.join(pdir, "unet", "diffusion_pytorch_model.bin"), "paint unet"),
                                     map_location="cpu", weights_only=True)
            names = [n for n, _ in unet.named_parameters()]
            missing = [n for n in names if n not in unet_sd]
            if missing:
                raise RuntimeError(f"paint unet checkpoint lacks {len(missing)} parameters, e.g. {missing[:3]}")
            for name in names:
                set_module_tensor_to_device(unet, name, device="cpu", dtype=torch.float16, value=unet_sd[name])
            unused = len(set(unet_sd) - set(names))
            del unet_sd
            vae = AutoencoderKL.from_pretrained(pdir, subfolder="vae", torch_dtype=torch.float16)
            # kijai's loader also loads text_encoder (CLIPTextModel, 1.4 GB), but HunyuanPaintPipeline never runs it
            # (prompt embeds = unet.learned_text_clip_gen).  Leaving it out also keeps diffusers' pipe.device /
            # _execution_device (first module of an unordered set) from resolving to a CPU-resident encoder.
            text_encoder = None
            tokenizer = CLIPTokenizer.from_pretrained(pdir, subfolder="tokenizer")
            feature_extractor = CLIPImageProcessor.from_pretrained(pdir, subfolder="feature_extractor")
            # the folder's scheduler is a DDIMScheduler (v_prediction, zero-terminal-SNR); upstream converts it:
            sched_base = DDIMScheduler.from_pretrained(pdir, subfolder="scheduler")
            if self.turbo:
                # upstream Multiview_Diffusion_Net for 'hunyuanpaint-turbo': LCMScheduler(trailing) + set_turbo(True)
                scheduler = LCMScheduler.from_config(sched_base.config, timestep_spacing="trailing")
            else:
                scheduler = EulerAncestralDiscreteScheduler.from_config(sched_base.config, timestep_spacing="trailing")
            self.paint = HunyuanPaintPipeline(unet=unet, vae=vae, text_encoder=text_encoder, tokenizer=tokenizer,
                                              scheduler=scheduler, feature_extractor=feature_extractor)
            self.paint.set_progress_bar_config(disable=False)
        log(f"texture: paint={args.paint} ({'turbo LCM' if self.turbo else 'EulerA'}; {unused} unused ckpt tensors), "
            f"delight={'off' if self.delight is None else args.delight}")
        if args.low_vram:
            if self.delight is not None:
                self.delight.enable_model_cpu_offload()
            self.paint.enable_model_cpu_offload()

    # -- pipelines on/off GPU -------------------------------------------------------------------------------
    def _to(self, pipe, device):
        """Move every nn.Module component (pipe.to('cpu') on an fp16 pipeline only adds a warning)."""
        import torch
        if self.args.low_vram:
            return
        for comp in pipe.components.values():
            if isinstance(comp, torch.nn.Module):
                comp.to(device)

    def delight_image(self, image):
        """Upstream Light_Shadow_Remover.__call__ (resize 512, eroded alpha, IP2P 50 steps, colour re-correction)."""
        import cv2
        import numpy as np
        import torch
        from PIL import Image
        dev = self.device
        image = image.resize((512, 512))
        if image.mode == "RGBA":
            arr = np.array(image)
            alpha = cv2.erode(arr[:, :, 3], np.ones((3, 3), np.uint8), iterations=1)
            arr[alpha == 0, :3] = 255
            arr[:, :, 3] = alpha
            image = Image.fromarray(arr)
            t = torch.tensor(np.array(image) / 255.0).to(dev)
            alpha_t, rgb_target = t[:, :, 3:], t[:, :, :3]
        else:
            t = torch.tensor(np.array(image.convert("RGB")) / 255.0).to(dev)
            alpha_t, rgb_target = torch.ones_like(t)[:, :, :1], t[:, :, :3]
        image = image.convert("RGB")
        self._to(self.delight, dev)
        try:
            out = self.delight(prompt="", image=image, generator=torch.manual_seed(42), height=512, width=512,
                               num_inference_steps=self.args.delight_steps, image_guidance_scale=1.5,
                               guidance_scale=1.0).images[0]
        finally:
            self._to(self.delight, "cpu")
            free_vram()
        src = torch.tensor(np.array(out) / 255.0).to(dev)[:, :, :3]
        img = recorrect_rgb(src, rgb_target, alpha_t)
        img = img[:, :, :3] * img[:, :, 3:] + torch.ones_like(img[:, :, :3]) * (1.0 - img[:, :, 3:])
        return Image.fromarray((img.cpu().numpy() * 255).astype(np.uint8))

    def multiview(self, prompt_rgb, normal_maps, position_maps, seed):
        """Upstream Multiview_Diffusion_Net.__call__ mapped onto the vendored HunyuanPaintPipeline API."""
        import numpy as np
        import torch
        view = 512
        ref = prompt_rgb.convert("RGB").resize((view, view))
        ref_t = torch.tensor(np.array(ref) / 255.0).permute(2, 0, 1)[None, None].to(self.device, torch.float16)
        controls = [im.resize((view, view)) for im in normal_maps + position_maps]
        for i, im in enumerate(controls):
            if im.mode == "L":
                controls[i] = im.point(lambda x: 255 if x > 1 else 0, mode="1")
        n = len(controls) // 2
        camera_info = [(((azim // 30) + 9) % 12) // {-20: 1, 0: 1, 20: 1, -90: 3, 90: 3}[elev]
                       + {-20: 0, 0: 12, 20: 24, -90: 36, 90: 40}[elev]
                       for azim, elev in zip(self.CAMERA_AZIMS, self.CAMERA_ELEVS)]
        kwargs = dict(width=view, height=view, num_in_batch=n, camera_info_gen=[camera_info], camera_info_ref=[[0]],
                      normal_imgs=[controls[:n]], position_imgs=[controls[n:]], denoise_strength=1.0,
                      generator=torch.Generator(device="cpu").manual_seed(seed), output_type="pil")
        if self.turbo:
            # upstream turbo: no CFG (and the turbo unet has no time_cond_proj_dim), 10 fixed LCM timesteps
            kwargs["guidance_scale"] = 1.0
            if self.args.paint_steps:
                kwargs["num_inference_steps"] = self.args.paint_steps
            else:
                kwargs["timesteps"] = list(self.TURBO_TIMESTEPS)
                kwargs["num_inference_steps"] = len(self.TURBO_TIMESTEPS)
        else:
            kwargs["guidance_scale"] = self.args.paint_guidance if self.args.paint_guidance is not None else 2.0
            kwargs["num_inference_steps"] = self.args.paint_steps or 30
        self._to(self.paint, self.device)
        try:
            with torch.no_grad():
                images = self.paint(ref_t, **kwargs).images
        finally:
            self._to(self.paint, "cpu")
            free_vram()
        return images

    @staticmethod
    def uv_wrap(mesh):
        """= texgen/utils/uv_warp_utils.mesh_uv_wrap, plus smooth normals carried over the seam splits."""
        import trimesh
        import xatlas
        if isinstance(mesh, trimesh.Scene):
            mesh = mesh.dump(concatenate=True)
        normals = mesh.vertex_normals.copy()
        vmapping, indices, uvs = xatlas.parametrize(mesh.vertices, mesh.faces)
        mesh.vertices = mesh.vertices[vmapping]
        mesh.faces = indices
        mesh.visual.uv = uvs
        return mesh, normals[vmapping]

    def generate(self, mesh, img_rgba, job, stats, debug_dir=None):
        import numpy as np
        import torch
        import trimesh
        a = self.args
        with stats.stage("texture: delight"):
            prompt = recenter_image(img_rgba, border_ratio=0.2)
            prompt = self.delight_image(prompt) if self.delight is not None else prompt.convert("RGB")
        with stats.stage("texture: uv wrap + render maps"):
            mesh, split_normals = self.uv_wrap(mesh)
            render = self.MeshRender(default_resolution=a.render_size, texture_size=a.texture_size)
            render.load_mesh(mesh)
            normal_maps = [render.render_normal(e, az, use_abs_coor=True, return_type="pl")[0]
                           for e, az in zip(self.CAMERA_ELEVS, self.CAMERA_AZIMS)]
            position_maps = [render.render_position(e, az, return_type="pl")
                             for e, az in zip(self.CAMERA_ELEVS, self.CAMERA_AZIMS)]
        with stats.stage("texture: multiview paint"):
            views = self.multiview(prompt, normal_maps, position_maps, job["seed"])
        with stats.stage("texture: bake + inpaint"):
            views_full = [v.resize((a.render_size, a.render_size)) for v in views]
            textures, cos_maps = [], []
            for view, elev, azim, weight in zip(views_full, self.CAMERA_ELEVS, self.CAMERA_AZIMS, self.VIEW_WEIGHTS):
                tex, cos_map, _ = render.back_project(view, elev, azim)
                textures.append(tex)
                cos_maps.append(weight * (cos_map ** self.BAKE_EXP))
            texture, trust = render.fast_bake_texture(textures, cos_maps)
            mask_np = ((trust > 1e-8).squeeze(-1).cpu().numpy() * 255).astype(np.uint8)
            texture_np = render.uv_inpaint(texture, mask_np)
            render.set_texture(torch.tensor(texture_np / 255).float().to(texture.device))
            textured = render.save_mesh()
            del textures, cos_maps, texture
        free_vram()
        if debug_dir:
            os.makedirs(debug_dir, exist_ok=True)
            prompt.save(os.path.join(debug_dir, "delit.png"))
            for i, (nm, pm, v) in enumerate(zip(normal_maps, position_maps, views)):
                nm.save(os.path.join(debug_dir, f"normal_{i}.png"))
                pm.save(os.path.join(debug_dir, f"position_{i}.png"))
                v.save(os.path.join(debug_dir, f"view_{i}.png"))
            from PIL import Image
            Image.fromarray(texture_np).save(os.path.join(debug_dir, "texture.png"))
        # glTF PBR material: base colour texture, non-metallic (glTF's default metallicFactor is 1.0)
        img = textured.visual.material.image
        pbr = trimesh.visual.material.PBRMaterial(baseColorTexture=img, metallicFactor=0.0, roughnessFactor=0.9,
                                                  name="hunyuan3d")
        textured.visual = trimesh.visual.TextureVisuals(uv=textured.visual.uv, material=pbr)
        textured.vertex_normals = split_normals
        return textured


# ----------------------------------------------------------------------------------------------------------------
# driver
# ----------------------------------------------------------------------------------------------------------------

def normalize_height(mesh, height):
    import numpy as np
    if height is None:
        return mesh
    bmin, bmax = mesh.bounds
    scale = float(height) / max(float(bmax[1] - bmin[1]), 1e-9)
    center = (bmin + bmax) / 2.0
    mesh.apply_translation([-center[0], -bmin[1], -center[2]])
    mesh.apply_scale(scale)
    return mesh


def export_glb(mesh, path):
    os.makedirs(os.path.dirname(os.path.abspath(path)) or ".", exist_ok=True)
    _ = mesh.vertex_normals  # make sure smooth normals are cached/exported
    mesh.export(path, file_type="glb", include_normals=True)


def build_jobs(args):
    defaults = dict(texture=args.texture, faces=args.faces, seed=args.seed, steps=args.steps, octree=args.octree,
                    guidance=args.guidance, height=args.height)
    if not args.batch:
        job = dict(defaults, image=os.path.abspath(args.image), out=os.path.abspath(args.out))
        return [job]
    with open(args.batch, "r", encoding="utf-8") as fh:
        data = json.load(fh)
    base = os.path.dirname(os.path.abspath(args.batch))
    if isinstance(data, dict):
        defaults.update({k: v for k, v in data.get("defaults", {}).items() if k in defaults})
        entries = data.get("jobs", [])
    else:
        entries = data
    out_dir = os.path.abspath(args.out) if args.out else os.path.join(base, "out")
    jobs = []
    for i, e in enumerate(entries):
        if isinstance(e, str):
            e = {"image": e}
        if "image" not in e:
            log(f"batch entry {i} has no 'image', skipped")
            continue
        image = e["image"] if os.path.isabs(e["image"]) else os.path.join(base, e["image"])
        name = e.get("name") or os.path.splitext(os.path.basename(image))[0]
        out = e.get("out") or e.get("output") or os.path.join(out_dir, f"{name}.glb")
        if not os.path.isabs(out):
            out = os.path.join(out_dir, out)
        job = dict(defaults)
        job.update({k: e[k] for k in defaults if k in e})
        job.update(image=image, out=out, name=name)
        jobs.append(job)
    return jobs


def run_check(args, pkg):
    import importlib

    import torch
    from hy3d_bootstrap import hy3d_import
    ok = True
    repo_dir = os.path.join(args.models, REPO)
    modules = ["hy3dgen.shapegen", "hy3dgen.shapegen.pipelines", "hy3dgen.shapegen.postprocessors",
               "hy3dgen.shapegen.models.autoencoders", "hy3dgen.texgen.differentiable_renderer.mesh_render",
               "hy3dgen.texgen.utils.uv_warp_utils", "hy3dgen.texgen.hunyuanpaint.pipeline",
               "hy3dgen.texgen.hunyuanpaint.unet.modules"]
    for m in modules:
        try:
            mod = hy3d_import(m)
            log(f"import OK   {m}  ({os.path.relpath(mod.__file__, args.wrapper_dir)})")
        except Exception as exc:
            ok = False
            log(f"import FAIL {m}: {exc!r}")
    for m in ("comfy.utils", "comfy.model_management", "folder_paths", "custom_rasterizer", "mesh_processor_fast"):
        mod = importlib.import_module(m)
        shim = os.path.normcase(mod.__file__).startswith(os.path.normcase(SHIM_DIR))
        ok &= shim
        log(f"{'shim OK  ' if shim else 'NOT SHIM '} {m}: {mod.__file__}")
    mp = sys.modules.get(f"{pkg}.hy3dgen.texgen.differentiable_renderer.mesh_processor")
    log(f"vendored mesh_processor resolves to: {getattr(mp, '__file__', None)} (replaced by numba port at runtime)")
    import custom_rasterizer as cr
    import nvdiffrast.torch as dr  # noqa: F401
    if torch.cuda.is_available():
        pos = torch.tensor([[[-1.0, -1.0, 0, 1], [1.0, -1.0, 0, 1], [-1.0, 1.0, 0, 1]]], device="cuda")
        tri = torch.tensor([[0, 1, 2]], dtype=torch.int32, device="cuda")
        f, b = cr.rasterize(pos, tri, (8, 8))
        log(f"custom_rasterizer shim rasterize OK (backend={cr.get_backend()}, covered {int((f > 0).sum())} px)")
    else:
        ok = False
        log("CUDA not available")
    want = {
        os.path.join(args.dit, "model.fp16.safetensors"): 4_930_777_530,
        os.path.join(args.dit, "config.yaml"): None,
        os.path.join(args.vae, "model.fp16.safetensors"): 407_410_402,
        os.path.join(args.vae, "config.yaml"): None,
        os.path.join(args.delight, "unet", "diffusion_pytorch_model.safetensors"): None,
        os.path.join(args.delight, "text_encoder", "model.safetensors"): None,
        os.path.join(args.delight, "vae", "diffusion_pytorch_model.safetensors"): None,
        os.path.join(args.paint, "unet", "diffusion_pytorch_model.safetensors"): 3_722_161_032,
        os.path.join(args.paint, "text_encoder", "pytorch_model.bin"): None,
        os.path.join(args.paint, "vae", "diffusion_pytorch_model.bin"): None,
    }
    for rel, size in want.items():
        path = os.path.join(repo_dir, rel)
        if not os.path.exists(path):
            log(f"weights MISSING  {rel}")
            ok = False
            continue
        got = os.path.getsize(path)
        good = size is None or got == size
        ok &= good
        log(f"weights {'OK     ' if good else 'BAD SIZE'} {rel} ({got / 2 ** 30:.2f} GiB)")
    log("CHECK " + ("PASSED" if ok else "FAILED"))
    return 0 if ok else 1


def main(argv=None):
    args = parse_args(argv)
    pkg, tmp = setup_environment(args)
    try:
        import torch
        from PIL import Image
        if args.check:
            return run_check(args, pkg)
        if not torch.cuda.is_available():
            raise RuntimeError("CUDA is required")
        device = torch.device("cuda")
        torch.backends.cuda.matmul.allow_tf32 = True
        repo_dir = os.path.join(args.models, REPO)
        jobs = build_jobs(args)
        log(f"{len(jobs)} job(s); models: {repo_dir}")
        load_stats = Stats()
        shape = ShapeGenerator(args, repo_dir, device, load_stats)
        texgen = None
        if any(j["texture"] for j in jobs):
            texgen = TextureGenerator(args, repo_dir, device, load_stats)
        load_stats.report("model loading")
        failures = 0
        for i, job in enumerate(jobs):
            stats = Stats()
            log(f"=== job {i + 1}/{len(jobs)}: {job['image']} -> {job['out']} "
                f"(texture={job['texture']}, faces={job['faces']}, seed={job['seed']}, steps={job['steps']}, "
                f"octree={job['octree']})")
            try:
                with stats.stage("load + background removal"):
                    img = Image.open(job["image"])
                    img.load()
                    if has_real_alpha(img) or args.keep_background:
                        rgba = img.convert("RGBA")
                    else:
                        rgba = shape.remove_background(img.convert("RGB")).convert("RGBA")
                debug_dir = None
                if args.debug_dir:
                    debug_dir = os.path.join(args.debug_dir, job.get("name") or
                                             os.path.splitext(os.path.basename(job["out"]))[0])
                    os.makedirs(debug_dir, exist_ok=True)
                    rgba.save(os.path.join(debug_dir, "input_rgba.png"))
                mesh = shape.generate(rgba, job, stats)
                if job["texture"]:
                    mesh = texgen.generate(mesh, rgba, job, stats, debug_dir)
                mesh = normalize_height(mesh, job.get("height"))
                with stats.stage("export glb"):
                    export_glb(mesh, job["out"])
                log(f"wrote {job['out']} ({os.path.getsize(job['out']) / 2 ** 20:.2f} MiB, "
                    f"{len(mesh.vertices)} verts, {len(mesh.faces)} faces, bounds {mesh.bounds.round(3).tolist()})")
            except Exception:
                failures += 1
                traceback.print_exc()
                log(f"job {i + 1} FAILED")
            finally:
                free_vram()
                stats.report(f"job {i + 1}")
        log(f"done: {len(jobs) - failures}/{len(jobs)} succeeded")
        return 1 if failures else 0
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
