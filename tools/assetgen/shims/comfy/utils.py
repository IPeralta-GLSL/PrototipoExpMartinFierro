"""Shim for comfy.utils: ProgressBar, load_torch_file, common_upscale."""

import torch


class ProgressBar:
    """No-op progress bar (ComfyUI UI hook); tqdm bars in the vendored code still print."""

    def __init__(self, total, node_id=None):
        self.total = total
        self.current = 0

    def update_absolute(self, value, total=None, preview=None):
        if total is not None:
            self.total = total
        self.current = value

    def update(self, value):
        self.current += value


def load_torch_file(ckpt, safe_load=False, device=None, return_metadata=False):
    if device is None:
        device = torch.device("cpu")
    metadata = None
    path = str(ckpt)
    if path.lower().endswith((".safetensors", ".sft")):
        import safetensors
        import safetensors.torch
        if return_metadata:
            with safetensors.safe_open(path, framework="pt", device="cpu") as handle:
                metadata = handle.metadata()
        sd = safetensors.torch.load_file(path, device=str(device))
    else:
        sd = torch.load(path, map_location=device, weights_only=True)
        if isinstance(sd, dict) and "state_dict" in sd:
            sd = sd["state_dict"]
    return (sd, metadata) if return_metadata else sd


def common_upscale(samples, width, height, upscale_method="bilinear", crop="disabled"):
    """Subset of comfy.utils.common_upscale (no crop, 'lanczos' -> bicubic+antialias)."""
    import torch.nn.functional as F
    mode = upscale_method
    if mode in ("lanczos", "bislerp", "area"):
        mode = "bicubic"
    kwargs = {"antialias": True} if mode in ("bilinear", "bicubic") else {}
    if mode in ("bilinear", "bicubic"):
        kwargs["align_corners"] = False
    return F.interpolate(samples, size=(height, width), mode=mode, **kwargs)
