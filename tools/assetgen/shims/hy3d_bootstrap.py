"""Make the Hunyuan3D-2 code vendored in ComfyUI-Hunyuan3DWrapper importable without ComfyUI.

Why a synthetic parent package is needed
---------------------------------------
The vendored code is written to live *inside* the ComfyUI custom-node package:
  * hy3dgen/shapegen/models/conditioner.py does ``from ....utils import log``
    (four dots -> the wrapper package's utils.py), so ``hy3dgen`` cannot be a
    top-level package;
  * hy3dgen/shapegen/pipelines.py resolves config targets such as
    ``.hy3dgen.shapegen.models.Hunyuan3DDiT`` with
    ``importlib.import_module(module, package=<basename of the wrapper dir>)``.
So we register an (empty, no __init__ executed) package whose name is the wrapper
directory's basename ("ComfyUI-Hunyuan3DWrapper") and whose __path__ is that
directory, exactly like ComfyUI does when it loads custom nodes.  The wrapper's
own __init__.py / nodes.py (which need the real ComfyUI) are never executed.

``comfy`` / ``folder_paths`` / ``custom_rasterizer`` are provided by the shims in
this directory, which must precede the ComfyUI root that the embedded python's
python311._pth puts on sys.path (the embedded python ignores PYTHONPATH).
"""

import importlib
import importlib.machinery
import importlib.util
import os
import sys

DEFAULT_WRAPPER_DIR = r"E:\ComfyUI\ComfyUIStudio\App\ComfyUI\custom_nodes\ComfyUI-Hunyuan3DWrapper"
SHIM_DIR = os.path.dirname(os.path.abspath(__file__))

_SHIMMED = ("comfy", "comfy.utils", "comfy.model_management", "folder_paths", "custom_rasterizer")
_state = {}


def _norm(path):
    return os.path.normcase(os.path.abspath(path))


def bootstrap(wrapper_dir=None, offline=True):
    """Prepare sys.path / sys.modules; returns the package name to import hy3dgen from."""
    if "pkg" in _state:
        return _state["pkg"]
    wrapper_dir = os.path.abspath(wrapper_dir or os.environ.get("HY3D_WRAPPER_DIR") or DEFAULT_WRAPPER_DIR)
    if not os.path.isdir(os.path.join(wrapper_dir, "hy3dgen")):
        raise FileNotFoundError(f"hy3dgen not found under wrapper dir: {wrapper_dir}")

    if offline:
        # every model is local; never let diffusers/transformers/hf_hub reach the network
        os.environ.setdefault("HF_HUB_OFFLINE", "1")
        os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")
        os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
    os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")

    # 1) shims first (shadow ComfyUI's comfy/ and folder_paths.py)
    sys.path[:] = [p for p in sys.path if _norm(p) != _norm(SHIM_DIR)]
    sys.path.insert(0, SHIM_DIR)
    for name in _SHIMMED:
        mod = sys.modules.get(name)
        if mod is not None and not _norm(getattr(mod, "__file__", "") or "").startswith(_norm(SHIM_DIR)):
            raise RuntimeError(f"module {name!r} was already imported from {mod.__file__}; "
                               "bootstrap() must run before anything imports it")

    # 2) wrapper dir on sys.path (appended so its utils.py / nodes.py never shadow anything)
    if _norm(wrapper_dir) not in {_norm(p) for p in sys.path}:
        sys.path.append(wrapper_dir)

    # 3) synthetic parent package named like the wrapper folder
    pkg = os.path.basename(wrapper_dir.rstrip("\\/"))
    if pkg not in sys.modules:
        spec = importlib.machinery.ModuleSpec(pkg, None, is_package=True)
        spec.submodule_search_locations = [wrapper_dir]
        module = importlib.util.module_from_spec(spec)
        module.__path__ = [wrapper_dir]
        sys.modules[pkg] = module
    _state["pkg"] = pkg
    _state["wrapper_dir"] = wrapper_dir
    return pkg


def hy3d_import(subpath):
    """hy3d_import('hy3dgen.shapegen') -> module imported through the wrapper package."""
    pkg = _state.get("pkg") or bootstrap()
    return importlib.import_module(f"{pkg}.{subpath}")
