"""Minimal stand-in for the ComfyUI ``comfy`` package.

The Hunyuan3D code vendored by ComfyUI-Hunyuan3DWrapper imports
``comfy.utils`` (ProgressBar, load_torch_file) and ``comfy.model_management``.
These shims provide just that surface so the code runs without ComfyUI.
This directory must come *before* the ComfyUI root on sys.path (the embedded
python's ._pth file adds ../ComfyUI), see hy3d_bootstrap.py.
"""

IS_HY3D_SHIM = True
