"""Shim for ComfyUI's folder_paths module (postprocessors.py uses temp_directory)."""

import os
import tempfile

_base = os.environ.get("HY3D_TEMP_DIR") or os.path.join(tempfile.gettempdir(), f"hy3dgen_{os.getpid()}")
temp_directory = _base
output_directory = os.path.join(_base, "output")
input_directory = os.path.join(_base, "input")
models_dir = os.environ.get("HY3DGEN_MODELS", os.path.join(_base, "models"))


def get_temp_directory():
    return temp_directory


def get_output_directory():
    return output_directory


def get_input_directory():
    return input_directory
