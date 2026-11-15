"""Fast drop-in for hy3dgen/texgen/differentiable_renderer/mesh_processor.meshVerticeInpaint.

On Python 3.11 the vendored folder's ``mesh_processor.cp312-win_amd64.pyd`` is not
importable (EXTENSION_SUFFIXES are .cp311-win_amd64.pyd/.pyd), so
``from .mesh_processor import meshVerticeInpaint`` loads the pure-python port
``mesh_processor.py``.  That port is complete but runs the Gauss-Seidel smoothing
loop with numpy scalars (seconds to minutes on real meshes).  This module is a
numba port of ``mesh_processor.cpp`` (same visiting order, same weights, float32
storage, C++ ``round`` = half away from zero); it falls back to the vendored
python function when numba is unavailable.
"""

import numpy as np

try:
    import numba

    _HAVE_NUMBA = True
except Exception:  # pragma: no cover
    numba = None
    _HAVE_NUMBA = False


def _texel(vtx_uv, vtx_uv_idx, texture_height, texture_width):
    # C++: int uv_v = round(uv.x * (W - 1)) [float]; int uv_u = round((1.0 - uv.y) * (H - 1)) [double]
    uv_v = int(np.floor(np.float32(vtx_uv[vtx_uv_idx, 0] * np.float32(texture_width - 1)) + 0.5))
    uv_u = int(np.floor((1.0 - np.float64(vtx_uv[vtx_uv_idx, 1])) * (texture_height - 1) + 0.5))
    # the C++ code does not bounds-check (UB for uv outside [0, 1]); clamp instead of corrupting memory
    uv_v = min(max(uv_v, 0), texture_width - 1)
    uv_u = min(max(uv_u, 0), texture_height - 1)
    return uv_u, uv_v


def _impl(texture, mask, vtx_pos, vtx_uv, pos_idx, uv_idx):
    texture_height, texture_width, texture_channel = texture.shape
    vtx_num = vtx_pos.shape[0]
    num_faces = uv_idx.shape[0]

    vtx_mask = np.zeros(vtx_num, dtype=np.float32)
    vtx_color = np.zeros((vtx_num, texture_channel), dtype=np.float32)
    uncolored = np.empty(num_faces * 3, dtype=np.int64)
    n_uncolored = 0

    # adjacency G (CSR, neighbours in insertion order exactly like vector<vector<int>> G)
    degree = np.zeros(vtx_num + 1, dtype=np.int64)
    for i in range(num_faces):
        for k in range(3):
            degree[pos_idx[i, k] + 1] += 1
    start = np.cumsum(degree)
    fill = start[:-1].copy()
    G = np.empty(num_faces * 3, dtype=np.int64)

    for i in range(num_faces):
        for k in range(3):
            vtx_uv_idx = uv_idx[i, k]
            vtx_idx = pos_idx[i, k]
            uv_u, uv_v = _texel(vtx_uv, vtx_uv_idx, texture_height, texture_width)
            if mask[uv_u, uv_v] > 0:
                vtx_mask[vtx_idx] = 1.0
                for c in range(texture_channel):
                    vtx_color[vtx_idx, c] = texture[uv_u, uv_v, c]
            else:
                uncolored[n_uncolored] = vtx_idx
                n_uncolored += 1
            G[fill[vtx_idx]] = pos_idx[i, (k + 1) % 3]
            fill[vtx_idx] += 1

    sum_color = np.zeros(texture_channel, dtype=np.float32)
    smooth_count = 2
    last_uncolored_vtx_count = 0
    while smooth_count > 0:
        uncolored_vtx_count = 0
        for e in range(n_uncolored):
            vtx_idx = uncolored[e]
            for c in range(texture_channel):
                sum_color[c] = 0.0
            total_weight = np.float32(0.0)
            for j in range(start[vtx_idx], start[vtx_idx + 1]):
                connected_idx = G[j]
                if vtx_mask[connected_idx] > 0:
                    # C++: pow(float diff, 2) -> difference in float, square/sqrt in double
                    dx = np.float64(vtx_pos[vtx_idx, 0] - vtx_pos[connected_idx, 0])
                    dy = np.float64(vtx_pos[vtx_idx, 1] - vtx_pos[connected_idx, 1])
                    dz = np.float64(vtx_pos[vtx_idx, 2] - vtx_pos[connected_idx, 2])
                    dist = np.sqrt(dx * dx + dy * dy + dz * dz)
                    dist_weight = np.float32(1.0 / max(dist, 1e-4))
                    dist_weight = np.float32(dist_weight * dist_weight)
                    for c in range(texture_channel):
                        sum_color[c] += vtx_color[connected_idx, c] * dist_weight
                    total_weight += dist_weight
            if total_weight > 0.0:
                for c in range(texture_channel):
                    vtx_color[vtx_idx, c] = sum_color[c] / total_weight
                vtx_mask[vtx_idx] = 1.0
            else:
                uncolored_vtx_count += 1
        if last_uncolored_vtx_count == uncolored_vtx_count:
            smooth_count -= 1
        else:
            smooth_count += 1
        last_uncolored_vtx_count = uncolored_vtx_count

    new_texture = texture.copy()
    new_mask = mask.copy()
    for face_idx in range(num_faces):
        for k in range(3):
            vtx_uv_idx = uv_idx[face_idx, k]
            vtx_idx = pos_idx[face_idx, k]
            if vtx_mask[vtx_idx] == 1.0:
                uv_u, uv_v = _texel(vtx_uv, vtx_uv_idx, texture_height, texture_width)
                for c in range(texture_channel):
                    new_texture[uv_u, uv_v, c] = vtx_color[vtx_idx, c]
                new_mask[uv_u, uv_v] = 255
    return new_texture, new_mask


if _HAVE_NUMBA:
    _texel = numba.njit(cache=True)(_texel)  # rebinding the global so the jitted _impl calls the jitted helper
    _kernel = numba.njit(cache=True, nogil=True)(_impl)
else:
    _kernel = None


def meshVerticeInpaint(texture, mask, vtx_pos, vtx_uv, pos_idx, uv_idx, method="smooth"):
    if method != "smooth":
        raise ValueError("Invalid method. Use 'smooth' or 'forward'.")
    texture = np.ascontiguousarray(texture, dtype=np.float32)
    mask = np.ascontiguousarray(mask, dtype=np.uint8)
    vtx_pos = np.ascontiguousarray(vtx_pos, dtype=np.float32)
    vtx_uv = np.ascontiguousarray(vtx_uv, dtype=np.float32)
    pos_idx = np.ascontiguousarray(pos_idx, dtype=np.int64)
    uv_idx = np.ascontiguousarray(uv_idx, dtype=np.int64)
    if _kernel is not None:
        return _kernel(texture, mask, vtx_pos, vtx_uv, pos_idx, uv_idx)
    return _impl(texture, mask, vtx_pos, vtx_uv, pos_idx, uv_idx)


def install(mesh_render_module):
    """Point mesh_render's ``meshVerticeInpaint`` (bound by ``from .mesh_processor import ...``) here."""
    mesh_render_module.meshVerticeInpaint = meshVerticeInpaint
    return mesh_render_module
