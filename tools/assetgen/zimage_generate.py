r"""
zimage_generate.py - local reference-image generator: Z-Image-Turbo (Tongyi-MAI) via a headless ComfyUI.

Run it with the ComfyUIStudio embedded Python (stdlib only; psutil is used when available):

  set PY=E:\ComfyUI\ComfyUIStudio\App\python_embeded\python.exe
  %PY% -B tools\assetgen\zimage_generate.py --name guitarra --prompt "a rustic guitar ..." --outdir tools\assetgen\test_out
  %PY% -B tools\assetgen\zimage_generate.py --jobs refs.json --outdir assets\refs
  %PY% -B tools\assetgen\zimage_generate.py --status
  %PY% -B tools\assetgen\zimage_generate.py --stop

Jobs file (JSON, UTF-8): a list - or {"jobs": [...]} - of objects
  {"name": "guitarra", "prompt": "...", "seed": 42, "width": 1024, "height": 1024}
  optional per job: steps, cfg, sampler, scheduler, shift, weight_dtype, negative (only used when cfg > 1)
  A bare string is accepted as {"prompt": "<string>"}.

Server: if nothing answers on 127.0.0.1:<port> the script starts Studio's ComfyUI 0.22.0 with exactly
the same command line as start_comfy.ps1 (see server_command()), waits for it, and leaves it running
so the next call reuses the loaded models. --stop shuts it down (after the jobs, if any were given).
The Studio install (E:\ComfyUI\ComfyUIStudio) and the model folder (E:\KRITA\ComfyUI\models) are only
read, never written: python -B, --base-directory comfy_runtime, in-memory database, ComfyUI-GGUF loaded
from comfy_runtime\custom_nodes (newer build with Qwen3 GGUF support, copied from Krita's ComfyUI).

Workflow (ComfyUI API format, see build_workflow()):
  UNETLoader(z_image_turbo_fp8_e4m3fn, weight_dtype fp8_e4m3fn) -> ModelSamplingAuraFlow(shift 3)
  -> KSampler(9 steps, cfg 1, res_multistep, simple) <- CLIPTextEncode(CLIPLoaderGGUF Qwen3-4B-Q4_K_M,
  type lumina2) + ConditioningZeroOut (negative) + EmptySD3LatentImage -> VAEDecode(flux_vae) -> SaveImage

Measured on the RTX 3080 Ti Laptop 16 GB (2026-11-14): ~18-20 s per 1024x1024 image once the models
are loaded (9 steps at ~1.8-2.0 s/it); the first image after a server start takes 50-80 s (model load);
peak GPU memory ~11.9 GB with fp8 weights, ~13 GB with weight_dtype "default".
Framing tip: tall/long objects get cropped in a square frame - use a portrait size such as 832x1216 and say
"full-length view of the whole <object> ... with empty white margin around it".
"""
import sys

sys.dont_write_bytecode = True  # never write .pyc files next to modules of the Studio install

import argparse
import json
import os
import random
import re
import shutil
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ASSETGEN_DIR = os.path.dirname(os.path.abspath(__file__))
RUNTIME_DIR = os.path.join(ASSETGEN_DIR, "comfy_runtime")
EXTRA_PATHS_YAML = os.path.join(ASSETGEN_DIR, "comfy_extra_model_paths.yaml")
PID_FILE = os.path.join(RUNTIME_DIR, "comfy_server.pid")
GGUF_NODE_DIR = os.path.join(RUNTIME_DIR, "custom_nodes", "ComfyUI-GGUF")
RUNTIME_SUBDIRS = ("custom_nodes", "input", "output", "temp", "user", "models", "logs")

STUDIO_APP = r"E:\ComfyUI\ComfyUIStudio\App"
STUDIO_PYTHON = os.path.join(STUDIO_APP, "python_embeded", "python.exe")
COMFY_MAIN = os.path.join(STUDIO_APP, "ComfyUI", "main.py")

HOST = "127.0.0.1"
DEFAULT_PORT = 8190

DEFAULT_UNET = "z_image_turbo_fp8_e4m3fn.safetensors"
DEFAULT_WEIGHT_DTYPE = "fp8_e4m3fn"  # same pixels as "default" (bf16 upcast) with ~5.8 GB less VRAM
DEFAULT_CLIP = "Qwen3-4B-Q4_K_M.gguf"
DEFAULT_CLIP_TYPE = "lumina2"  # Qwen3-4B + any type except flux/flux2 -> Z-Image text encoder in comfy/sd.py
DEFAULT_VAE = "flux_vae.safetensors"
SAVE_NODE_ID = "10"

CREATE_NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0x08000000)
CREATE_NEW_PROCESS_GROUP = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0x00000200)
CREATE_BREAKAWAY_FROM_JOB = 0x01000000


class ComfyError(RuntimeError):
    pass


def log(msg):
    print(msg, flush=True)


# --------------------------------------------------------------------------------------------- HTTP
class Comfy:
    def __init__(self, port):
        self.port = port
        self.base = f"http://{HOST}:{port}"
        # never route localhost traffic through a system proxy
        self._opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def _open(self, req, timeout):
        return self._opener.open(req, timeout=timeout)

    def get_json(self, path, timeout=30):
        with self._open(self.base + path, timeout) as r:
            return json.loads(r.read().decode("utf-8"))

    def get_bytes(self, path, timeout=120):
        with self._open(self.base + path, timeout) as r:
            return r.read()

    def post_json(self, path, payload, timeout=30):
        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(self.base + path, data=data, method="POST",
                                     headers={"Content-Type": "application/json"})
        try:
            with self._open(req, timeout) as r:
                body = r.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            body = e.read().decode("utf-8", "replace")
            raise ComfyError(f"POST {path} -> HTTP {e.code}: {body}") from None
        try:
            return json.loads(body) if body.strip() else {}
        except ValueError:
            return {"raw": body}

    def is_up(self):
        try:
            self.get_json("/system_stats", timeout=3)
            return True
        except Exception:
            return False


# ------------------------------------------------------------------------------------------ server
def server_command(port, python=STUDIO_PYTHON, main_py=COMFY_MAIN, extra=()):
    """The one and only launch command line (start_comfy.ps1 builds the same one)."""
    return [
        python, "-B", "-s", "-W", "ignore::FutureWarning", main_py,
        "--listen", HOST,
        "--port", str(port),
        "--base-directory", RUNTIME_DIR,
        "--extra-model-paths-config", EXTRA_PATHS_YAML,
        "--disable-all-custom-nodes",
        "--whitelist-custom-nodes", "ComfyUI-GGUF",
        "--disable-api-nodes",
        "--disable-auto-launch",
        "--database-url", "sqlite:///:memory:",
        "--preview-method", "none",
        "--log-stdout",
        *extra,
    ]


def quote_cmd(cmd):
    return subprocess.list2cmdline(cmd)


def ensure_runtime(python, main_py):
    for f in (python, main_py, EXTRA_PATHS_YAML):
        if not os.path.isfile(f):
            raise ComfyError(f"missing file: {f}")
    for d in RUNTIME_SUBDIRS:
        os.makedirs(os.path.join(RUNTIME_DIR, d), exist_ok=True)
    if not os.path.isfile(os.path.join(GGUF_NODE_DIR, "nodes.py")):
        raise ComfyError(f"ComfyUI-GGUF missing in {GGUF_NODE_DIR} "
                         r"(copy it from E:\KRITA\ComfyUI\ComfyUI\custom_nodes\ComfyUI-GGUF)")


def port_in_use(port):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(1.0)
        return s.connect_ex((HOST, port)) == 0


def listener_pid(port):
    try:
        import psutil
        for c in psutil.net_connections(kind="tcp"):
            if c.status == psutil.CONN_LISTEN and c.laddr and c.laddr.port == port and c.pid:
                return c.pid
    except Exception:
        pass
    try:  # fallback; the state column of netstat is localized, so match the foreign address instead
        out = subprocess.run(["netstat", "-ano", "-p", "TCP"], capture_output=True, text=True,
                             timeout=60, creationflags=CREATE_NO_WINDOW).stdout
        for line in out.splitlines():
            parts = line.split()
            if (len(parts) >= 5 and parts[0].upper() == "TCP" and parts[1].endswith(f":{port}")
                    and parts[2] in ("0.0.0.0:0", "[::]:0")):
                return int(parts[-1])
    except Exception:
        pass
    return None


def process_cmdline(pid):
    try:
        import psutil
        return " ".join(psutil.Process(pid).cmdline())
    except Exception:
        pass
    try:
        out = subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             f"(Get-CimInstance Win32_Process -Filter 'ProcessId={int(pid)}').CommandLine"],
            capture_output=True, text=True, timeout=60, creationflags=CREATE_NO_WINDOW).stdout
        return out.strip()
    except Exception:
        return ""


def is_our_comfy(cmdline):
    cl = cmdline.lower()
    return "main.py" in cl and "comfy_runtime" in cl


def read_pid_file():
    try:
        with open(PID_FILE, "r", encoding="ascii") as f:
            return int(f.read().strip().splitlines()[0])
    except Exception:
        return None


def remove_pid_file():
    try:
        os.remove(PID_FILE)
    except OSError:
        pass


def tail(path, n=40):
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            return "".join(f.readlines()[-n:])
    except OSError:
        return ""


def start_server(comfy, args):
    """Start ComfyUI unless it already answers. Returns True if this call started it."""
    if comfy.is_up():
        log(f"[server] reusing ComfyUI on {comfy.base} (PID {listener_pid(comfy.port)})")
        return False
    if port_in_use(comfy.port):
        raise ComfyError(f"port {comfy.port} is in use (PID {listener_pid(comfy.port)}) but does not answer "
                         "like ComfyUI; use another --port or stop that process")
    ensure_runtime(args.comfy_python, args.comfy_main)
    cmd = server_command(comfy.port, args.comfy_python, args.comfy_main, args.server_arg or ())
    stamp = time.strftime("%Y%m%d_%H%M%S")
    log_path = os.path.join(RUNTIME_DIR, "logs", f"comfyui_{stamp}.log")
    err_path = os.path.join(RUNTIME_DIR, "logs", f"comfyui_{stamp}.err.log")
    log(f"[server] starting: {quote_cmd(cmd)}")
    t0 = time.time()
    with open(log_path, "ab") as out, open(err_path, "ab") as err:
        popen_kw = dict(cwd=RUNTIME_DIR, stdin=subprocess.DEVNULL, stdout=out, stderr=err, close_fds=True)
        flags = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
        try:  # outlive the caller even if it runs inside a kill-on-close job object
            proc = subprocess.Popen(cmd, creationflags=flags | CREATE_BREAKAWAY_FROM_JOB, **popen_kw)
        except OSError:
            proc = subprocess.Popen(cmd, creationflags=flags, **popen_kw)
    with open(PID_FILE, "w", encoding="ascii") as f:
        f.write(str(proc.pid))
    log(f"[server] PID {proc.pid}, log {log_path}")
    deadline = t0 + args.start_timeout
    while time.time() < deadline:
        rc = proc.poll()
        if rc is not None:
            remove_pid_file()
            raise ComfyError(f"ComfyUI exited during startup (code {rc}). Log tail:\n"
                             f"{tail(log_path)}\n{tail(err_path)}")
        if comfy.is_up():
            log(f"[server] ready on {comfy.base} after {time.time() - t0:.1f} s")
            return True
        time.sleep(1.0)
    raise ComfyError(f"ComfyUI did not answer within {args.start_timeout} s (PID {proc.pid} still running; "
                     f"see {log_path})")


def kill_tree(pid):
    subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], capture_output=True, text=True,
                   creationflags=CREATE_NO_WINDOW)


def stop_server(comfy, force=False):
    """Stop the ComfyUI listening on the port (and/or the PID in the pid file) if it is ours."""
    targets = []
    pid = listener_pid(comfy.port)
    if pid:
        cl = process_cmdline(pid)
        if is_our_comfy(cl) or force:
            targets.append(pid)
        else:
            log(f"[server] port {comfy.port} belongs to PID {pid}, which is not this ComfyUI "
                f"({cl[:160]!r}); not stopping it (use --force-stop to override)")
            return False
    file_pid = read_pid_file()
    if file_pid and file_pid not in targets:
        cl = process_cmdline(file_pid)
        if cl and is_our_comfy(cl):
            targets.append(file_pid)
    if not targets:
        log(f"[server] nothing to stop on port {comfy.port}")
        remove_pid_file()
        return True
    try:
        comfy.post_json("/interrupt", {}, timeout=3)
    except Exception:
        pass
    for p in targets:
        log(f"[server] stopping PID {p}")
        kill_tree(p)
    for _ in range(120):
        if not port_in_use(comfy.port):
            break
        time.sleep(0.25)
    remove_pid_file()
    if port_in_use(comfy.port):
        log(f"[server] WARNING: port {comfy.port} is still in use")
        return False
    log(f"[server] stopped; port {comfy.port} is free")
    return True


# ---------------------------------------------------------------------------------------- workflow
def build_workflow(job):
    """ComfyUI API-format workflow for one Z-Image-Turbo text-to-image job."""
    negative_text = (job.get("negative") or "").strip()
    if negative_text and float(job["cfg"]) > 1.0:
        negative = {"class_type": "CLIPTextEncode", "inputs": {"clip": ["2", 0], "text": negative_text},
                    "_meta": {"title": "Negative prompt"}}
    else:  # cfg 1.0 -> negative is not evaluated; zeroed conditioning as in the official template
        negative = {"class_type": "ConditioningZeroOut", "inputs": {"conditioning": ["5", 0]},
                    "_meta": {"title": "Negative (zeroed)"}}
    return {
        "1": {"class_type": "UNETLoader",
              "inputs": {"unet_name": job["unet"], "weight_dtype": job["weight_dtype"]},
              "_meta": {"title": "Load Z-Image-Turbo"}},
        "2": {"class_type": "CLIPLoaderGGUF",
              "inputs": {"clip_name": job["clip"], "type": job["clip_type"]},
              "_meta": {"title": "Load Qwen3-4B text encoder (GGUF)"}},
        "3": {"class_type": "VAELoader",
              "inputs": {"vae_name": job["vae"]},
              "_meta": {"title": "Load VAE"}},
        "4": {"class_type": "ModelSamplingAuraFlow",
              "inputs": {"model": ["1", 0], "shift": float(job["shift"])},
              "_meta": {"title": "ModelSamplingAuraFlow"}},
        "5": {"class_type": "CLIPTextEncode",
              "inputs": {"clip": ["2", 0], "text": job["prompt"]},
              "_meta": {"title": "Positive prompt"}},
        "6": negative,
        "7": {"class_type": "EmptySD3LatentImage",
              "inputs": {"width": int(job["width"]), "height": int(job["height"]), "batch_size": 1},
              "_meta": {"title": "Empty latent"}},
        "8": {"class_type": "KSampler",
              "inputs": {"model": ["4", 0], "seed": int(job["seed"]), "steps": int(job["steps"]),
                         "cfg": float(job["cfg"]), "sampler_name": job["sampler"],
                         "scheduler": job["scheduler"], "positive": ["5", 0], "negative": ["6", 0],
                         "latent_image": ["7", 0], "denoise": 1.0},
              "_meta": {"title": "KSampler"}},
        "9": {"class_type": "VAEDecode",
              "inputs": {"samples": ["8", 0], "vae": ["3", 0]},
              "_meta": {"title": "VAE Decode"}},
        SAVE_NODE_ID: {"class_type": "SaveImage",
                       "inputs": {"images": ["9", 0], "filename_prefix": f"zimage/{job['name']}"},
                       "_meta": {"title": "Save Image"}},
    }


def enum_options(spec):
    if not spec:
        return None
    if isinstance(spec[0], list):
        return spec[0]
    if spec[0] == "COMBO" and len(spec) > 1 and isinstance(spec[1], dict):
        return spec[1].get("options")
    return None


def validate_workflow(comfy, wf, cache):
    """Check class names, input names, enum values and numeric ranges against GET /object_info."""
    problems = []
    for node_id, node in wf.items():
        ct = node["class_type"]
        if ct not in cache:
            try:
                cache[ct] = comfy.get_json(f"/object_info/{urllib.parse.quote(ct)}").get(ct)
            except Exception as e:
                cache[ct] = None
                problems.append(f"node {node_id}: cannot query /object_info/{ct}: {e}")
                continue
        info = cache[ct]
        if info is None:
            problems.append(f"node {node_id}: node class '{ct}' is not available on the server")
            continue
        spec_in = info.get("input", {})
        required = spec_in.get("required", {})
        known = {**required, **spec_in.get("optional", {})}
        for name in required:
            if name not in node["inputs"]:
                problems.append(f"node {node_id} ({ct}): missing required input '{name}'")
        for name, value in node["inputs"].items():
            spec = known.get(name)
            if spec is None:
                problems.append(f"node {node_id} ({ct}): unknown input '{name}'")
                continue
            if isinstance(value, list):  # link to another node's output
                continue
            options = enum_options(spec)
            if options is not None:
                if value not in options:
                    problems.append(f"node {node_id} ({ct}): {name}={value!r} not in {options}")
                continue
            if spec[0] in ("INT", "FLOAT") and len(spec) > 1 and isinstance(spec[1], dict):
                lo, hi = spec[1].get("min"), spec[1].get("max")
                if (lo is not None and value < lo) or (hi is not None and value > hi):
                    problems.append(f"node {node_id} ({ct}): {name}={value} outside [{lo}, {hi}]")
    return problems


# -------------------------------------------------------------------------------------------- jobs
def safe_name(name):
    name = re.sub(r"[^A-Za-z0-9_.-]+", "_", str(name)).strip("._")
    return name or "image"


def load_jobs(args):
    raw = []
    if args.jobs:
        with open(args.jobs, "r", encoding="utf-8-sig") as f:
            data = json.load(f)
        if isinstance(data, dict):
            data = data.get("jobs", [])
        if not isinstance(data, list):
            raise ComfyError("jobs file must contain a list of jobs or {\"jobs\": [...]}")
        for i, j in enumerate(data):
            if isinstance(j, str):
                j = {"prompt": j}
            if not isinstance(j, dict) or not str(j.get("prompt", "")).strip():
                raise ComfyError(f"job #{i} in {args.jobs} has no prompt")
            raw.append(dict(j))
    for i, p in enumerate(args.prompt or []):
        j = {"prompt": p}
        base = args.name or f"zimage_{time.strftime('%Y%m%d_%H%M%S')}"
        j["name"] = base if len(args.prompt) == 1 else f"{base}_{i + 1:02d}"
        if args.seed is not None:
            j["seed"] = args.seed + i
        if args.negative:
            j["negative"] = args.negative
        raw.append(j)

    jobs, used = [], set()
    for i, j in enumerate(raw):
        job = {
            "name": safe_name(j.get("name") or f"image_{i + 1:03d}"),
            "prompt": str(j["prompt"]).strip(),
            "negative": j.get("negative", ""),
            "seed": j.get("seed"),
            "width": int(j.get("width", args.width)),
            "height": int(j.get("height", args.height)),
            "steps": int(j.get("steps", args.steps)),
            "cfg": float(j.get("cfg", args.cfg)),
            "sampler": j.get("sampler", args.sampler),
            "scheduler": j.get("scheduler", args.scheduler),
            "shift": float(j.get("shift", args.shift)),
            "weight_dtype": j.get("weight_dtype", args.weight_dtype),
            "unet": args.unet, "clip": args.clip, "clip_type": args.clip_type, "vae": args.vae,
        }
        if job["seed"] is None:
            job["seed"] = random.randint(0, 2**31 - 1)
        job["seed"] = int(job["seed"])
        for dim in ("width", "height"):
            if job[dim] % 16:
                raise ComfyError(f"job {job['name']}: {dim}={job[dim]} must be a multiple of 16")
        base, k = job["name"], 2
        while job["name"].lower() in used:
            job["name"] = f"{base}_{k}"
            k += 1
        used.add(job["name"].lower())
        jobs.append(job)
    return jobs


def wait_for(comfy, prompt_id, timeout):
    deadline = time.time() + timeout
    failures = 0
    while True:
        try:
            entry = comfy.get_json(f"/history/{prompt_id}", timeout=30).get(prompt_id)
            failures = 0
        except (urllib.error.URLError, ConnectionError, socket.timeout) as e:
            failures += 1
            if failures >= 10 or not port_in_use(comfy.port):
                raise ComfyError(f"lost connection to ComfyUI while waiting for {prompt_id}: {e}")
            entry = None
        if entry and entry.get("status", {}).get("status_str") in ("success", "error"):
            return entry
        if entry and entry.get("outputs") and entry.get("status", {}).get("completed"):
            return entry
        if time.time() > deadline:
            raise ComfyError(f"timed out after {timeout} s waiting for prompt {prompt_id}")
        time.sleep(0.5)


def exec_seconds(entry):
    start = end = None
    for ev, data in entry.get("status", {}).get("messages", []):
        if ev == "execution_start":
            start = data.get("timestamp")
        elif ev in ("execution_success", "execution_error", "execution_interrupted"):
            end = data.get("timestamp")
    if start is not None and end is not None:
        return (end - start) / 1000.0
    return None


def exec_error(entry):
    for ev, data in entry.get("status", {}).get("messages", []):
        if ev == "execution_error":
            return (f"{data.get('node_type')} (node {data.get('node_id')}): "
                    f"{data.get('exception_type')}: {data.get('exception_message')}")
        if ev == "execution_interrupted":
            return "execution interrupted"
    return "unknown error (see server log)"


def fetch_images(comfy, entry, outdir, name):
    images = [img for img in entry.get("outputs", {}).get(SAVE_NODE_ID, {}).get("images", [])
              if img.get("type") == "output"]
    files = []
    for idx, img in enumerate(images):
        query = urllib.parse.urlencode({"filename": img["filename"], "subfolder": img.get("subfolder", ""),
                                        "type": img.get("type", "output")})
        data = comfy.get_bytes(f"/view?{query}")
        ext = os.path.splitext(img["filename"])[1] or ".png"
        dest = os.path.join(outdir, (name if idx == 0 else f"{name}_{idx + 1}") + ext)
        tmp = dest + ".part"
        with open(tmp, "wb") as f:
            f.write(data)
        os.replace(tmp, dest)
        files.append(dest)
    return files


class VramMonitor:
    """Samples total GPU memory in use (nvidia-smi) in the background to report the peak."""

    def __init__(self, interval=0.5):
        self.exe = shutil.which("nvidia-smi")
        self.interval = interval
        self.peak = None
        self.total = None
        self._stop = threading.Event()
        self._thread = None

    def sample(self):
        if not self.exe:
            return None
        try:
            out = subprocess.run([self.exe, "--query-gpu=memory.used,memory.total",
                                  "--format=csv,noheader,nounits", "--id=0"],
                                 capture_output=True, text=True, timeout=10,
                                 creationflags=CREATE_NO_WINDOW).stdout
            used, total = (int(float(x)) for x in out.strip().splitlines()[0].split(","))
            self.total = total
            return used
        except Exception:
            return None

    def _run(self):
        while not self._stop.is_set():
            used = self.sample()
            if used is not None:
                self.peak = used if self.peak is None else max(self.peak, used)
            self._stop.wait(self.interval)

    def start(self):
        if self.exe:
            self._thread = threading.Thread(target=self._run, daemon=True)
            self._thread.start()

    def stop(self):
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=15)


def run_jobs(comfy, jobs, args):
    outdir = os.path.abspath(args.outdir)
    os.makedirs(outdir, exist_ok=True)

    cache, prepared = {}, []
    for job in jobs:
        if args.skip_existing and os.path.exists(os.path.join(outdir, job["name"] + ".png")):
            log(f"[skip] {job['name']}: already exists in {outdir}")
            continue
        wf = build_workflow(job)
        problems = validate_workflow(comfy, wf, cache)
        if problems:
            log(f"[error] workflow for {job['name']} does not match the server's /object_info:")
            for p in problems:
                log(f"   - {p}")
            return 2
        prepared.append((job, wf))
    if not prepared:
        log("[info] nothing to generate")
        return 0

    monitor = VramMonitor() if args.vram_monitor else None
    vram_before = monitor.sample() if monitor else None
    if monitor:
        monitor.start()

    client_id = uuid.uuid4().hex
    t_batch = time.time()
    queued = []
    for job, wf in prepared:
        resp = comfy.post_json("/prompt", {"prompt": wf, "client_id": client_id})
        if resp.get("node_errors"):
            log(f"[error] {job['name']}: node errors: {json.dumps(resp['node_errors'])}")
            continue
        queued.append((job, resp["prompt_id"]))
        log(f"[queued] {job['name']}  seed={job['seed']}  {job['width']}x{job['height']}  "
            f"prompt_id={resp['prompt_id']}")

    results, failed = [], 0
    manifest = os.path.join(outdir, "zimage_manifest.jsonl")
    try:
        for job, prompt_id in queued:
            entry = wait_for(comfy, prompt_id, args.job_timeout)
            secs = exec_seconds(entry)
            if entry.get("status", {}).get("status_str") != "success":
                failed += 1
                log(f"[failed] {job['name']}: {exec_error(entry)}")
                continue
            files = fetch_images(comfy, entry, outdir, job["name"])
            if not files:
                failed += 1
                log(f"[failed] {job['name']}: finished but produced no image")
                continue
            log(f"[done] {job['name']}  {secs:.1f} s  -> {files[0]}" if secs is not None
                else f"[done] {job['name']} -> {files[0]}")
            rec = {k: job[k] for k in ("name", "prompt", "negative", "seed", "width", "height", "steps",
                                       "cfg", "sampler", "scheduler", "shift", "weight_dtype", "unet",
                                       "clip", "vae")}
            rec.update(files=files, seconds=secs, prompt_id=prompt_id,
                       created=time.strftime("%Y-%m-%dT%H:%M:%S"))
            with open(manifest, "a", encoding="utf-8") as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
            results.append(rec)
    finally:
        if monitor:
            monitor.stop()

    wall = time.time() - t_batch
    times = [r["seconds"] for r in results if r["seconds"] is not None]
    log(f"[summary] {len(results)} ok, {failed + len(prepared) - len(queued)} failed, "
        f"wall {wall:.1f} s" + (f", per image: {', '.join(f'{t:.1f}' for t in times)} s" if times else ""))
    if monitor and monitor.peak is not None:
        log(f"[vram] GPU memory in use (all processes): before {vram_before} MiB, peak {monitor.peak} MiB, "
            f"total {monitor.total} MiB")
    try:
        dev = comfy.get_json("/system_stats", timeout=10)["devices"][0]
        log(f"[vram] after run: free {dev['vram_free'] / 2**20:.0f} of {dev['vram_total'] / 2**20:.0f} MiB; "
            f"torch reserved {dev['torch_vram_total'] / 2**20:.0f} MiB")
    except Exception:
        pass
    return 0 if failed == 0 and len(queued) == len(prepared) else 1


def print_status(comfy):
    if not comfy.is_up():
        log(f"[status] ComfyUI is NOT running on {comfy.base}"
            + (f" (port in use by PID {listener_pid(comfy.port)})" if port_in_use(comfy.port) else ""))
        return
    stats = comfy.get_json("/system_stats")
    queue = comfy.get_json("/queue")
    dev = stats["devices"][0]
    log(f"[status] ComfyUI {stats['system'].get('comfyui_version')} running on {comfy.base} "
        f"(PID {listener_pid(comfy.port)}); queue running={len(queue.get('queue_running', []))} "
        f"pending={len(queue.get('queue_pending', []))}; VRAM free {dev['vram_free'] / 2**20:.0f}/"
        f"{dev['vram_total'] / 2**20:.0f} MiB, torch reserved {dev['torch_vram_total'] / 2**20:.0f} MiB")


# -------------------------------------------------------------------------------------------- main
def parse_args(argv=None):
    p = argparse.ArgumentParser(description="Generate images with Z-Image-Turbo through a local headless ComfyUI.")
    g = p.add_argument_group("jobs")
    g.add_argument("--prompt", action="append", help="prompt text (repeatable)")
    g.add_argument("--name", help="output base name for --prompt jobs")
    g.add_argument("--seed", type=int, help="seed for --prompt jobs (random if omitted)")
    g.add_argument("--negative", help="negative prompt (only has an effect with --cfg > 1)")
    g.add_argument("--jobs", help="JSON file with a list of {name, prompt, seed, width, height}")
    g.add_argument("--outdir", default=os.path.join(ASSETGEN_DIR, "out"), help="where results are copied")
    g.add_argument("--skip-existing", action="store_true", help="skip jobs whose <name>.png already exists")
    s = p.add_argument_group("sampling")
    s.add_argument("--width", type=int, default=1024)
    s.add_argument("--height", type=int, default=1024)
    s.add_argument("--steps", type=int, default=9)
    s.add_argument("--cfg", type=float, default=1.0)
    s.add_argument("--sampler", default="res_multistep")
    s.add_argument("--scheduler", default="simple")
    s.add_argument("--shift", type=float, default=3.0)
    s.add_argument("--weight-dtype", default=DEFAULT_WEIGHT_DTYPE,
                   help="UNETLoader weight_dtype. 'fp8_e4m3fn' (default) keeps the fp8 file fp8 in VRAM (~5.9 GB) "
                        "and casts per layer to bf16; 'default' upcasts it to bf16 at load (~11.7 GB, ~8%% faster). "
                        "Both give bit-identical images.")
    s.add_argument("--unet", default=DEFAULT_UNET)
    s.add_argument("--clip", default=DEFAULT_CLIP)
    s.add_argument("--clip-type", default=DEFAULT_CLIP_TYPE)
    s.add_argument("--vae", default=DEFAULT_VAE)
    v = p.add_argument_group("server")
    v.add_argument("--port", type=int, default=DEFAULT_PORT)
    v.add_argument("--start", action="store_true", help="make sure the server is running, then exit")
    v.add_argument("--stop", action="store_true", help="stop the server (after the jobs, if any)")
    v.add_argument("--force-stop", action="store_true", help="with --stop: kill whatever listens on --port")
    v.add_argument("--status", action="store_true", help="print server status")
    v.add_argument("--no-start", action="store_true", help="fail instead of starting the server")
    v.add_argument("--start-timeout", type=int, default=300)
    v.add_argument("--job-timeout", type=int, default=1800)
    v.add_argument("--server-arg", action="append", help="extra ComfyUI argument when starting (repeatable)")
    v.add_argument("--comfy-python", default=STUDIO_PYTHON)
    v.add_argument("--comfy-main", default=COMFY_MAIN)
    v.add_argument("--print-launch", action="store_true", help="print the server command line and exit")
    v.add_argument("--dump-workflow", metavar="FILE", help="write the API workflow of the first job to FILE")
    v.add_argument("--dry-run", action="store_true", help="build (and dump) the workflow, do not contact the server")
    v.add_argument("--no-vram-monitor", dest="vram_monitor", action="store_false")
    return p, p.parse_args(argv)


def main(argv=None):
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(errors="replace")
        except Exception:
            pass
    parser, args = parse_args(argv)
    comfy = Comfy(args.port)

    if args.print_launch:
        log(quote_cmd(server_command(args.port, args.comfy_python, args.comfy_main, args.server_arg or ())))
        return 0

    jobs = load_jobs(args)
    if not (jobs or args.start or args.stop or args.status or args.dump_workflow):
        parser.error("nothing to do: give --prompt, --jobs, --start, --stop, --status or --dump-workflow")

    if args.dump_workflow:
        sample = jobs[0] if jobs else load_jobs(parse_args(["--prompt", "PROMPT", "--name", "NAME",
                                                             "--seed", "0"])[1])[0]
        with open(args.dump_workflow, "w", encoding="utf-8") as f:
            json.dump(build_workflow(sample), f, indent=2, ensure_ascii=False)
        log(f"[workflow] written to {os.path.abspath(args.dump_workflow)}")
    if args.dry_run:
        for job in jobs:
            log(f"[dry-run] {job['name']}: seed={job['seed']} {job['width']}x{job['height']} "
                f"steps={job['steps']} cfg={job['cfg']} {job['sampler']}/{job['scheduler']} "
                f"shift={job['shift']} weight_dtype={job['weight_dtype']}")
        return 0

    rc = 0
    try:
        if args.status:
            print_status(comfy)
        if jobs or args.start:
            if not comfy.is_up() and args.no_start:
                raise ComfyError(f"ComfyUI is not running on {comfy.base} and --no-start was given")
            start_server(comfy, args)
        if jobs:
            rc = run_jobs(comfy, jobs, args)
    except ComfyError as e:
        log(f"[error] {e}")
        rc = 1
    except KeyboardInterrupt:
        log("[interrupted] the server keeps running; use --stop to shut it down")
        rc = 130
    finally:
        if args.stop:
            if not stop_server(comfy, force=args.force_stop):
                rc = rc or 1
    return rc


if __name__ == "__main__":
    sys.exit(main())
