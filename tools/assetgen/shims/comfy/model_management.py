"""Shim for comfy.model_management (only what the vendored Hunyuan3D code touches)."""

import gc

import torch


def get_torch_device():
    return torch.device("cuda") if torch.cuda.is_available() else torch.device("cpu")


def unet_offload_device():
    return torch.device("cpu")


def text_encoder_offload_device():
    return torch.device("cpu")


def intermediate_device():
    return torch.device("cpu")


def soft_empty_cache(force=False):
    gc.collect()
    if torch.cuda.is_available():
        torch.cuda.empty_cache()
        torch.cuda.ipc_collect()


def unload_all_models():
    soft_empty_cache()


def throw_exception_if_processing_interrupted():
    return None
