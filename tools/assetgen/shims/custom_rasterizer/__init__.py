"""Drop-in replacement for Hunyuan3D-2's ``custom_rasterizer`` CUDA extension.

The original package (hy3dgen/texgen/custom_rasterizer) is a compiled CUDA
extension that only ships as cp312 wheels.  This module re-implements its public
Python API on top of nvdiffrast (prebuilt) plus plain torch, following the
kernel sources (lib/custom_rasterizer_kernel/rasterizer.h, rasterizer.cpp,
rasterizer_gpu.cu) convention by convention:

* ``rasterize(pos, tri, resolution)``: only ``pos[0]`` is used (batch 0),
  ``resolution = (H, W)``; the kernel is called with width=resolution[1],
  height=resolution[0].  Returns ``findices`` int32 [H, W] (face index + 1,
  0 = background) and ``barycentric`` float32 [H, W, 3] (zeros on background).
* Screen mapping: NDC [-1, 1] maps to pixel *centres* 0 .. W-1 (not edges):
  ``sx = (x/w*0.5+0.5)*(W-1)+0.5``, ``sy = (0.5+0.5*y/w)*(H-1)+0.5``; pixel
  (px, py) is sampled at (px+0.5, py+0.5).  Row 0 is NDC y = -1 (no y flip).
* Coverage: all three 2D screen-space barycentrics in [0, 1] (inclusive),
  no back-face culling, degenerate (zero signed area) faces never cover.
* Visibility: smallest ``z/w*0.49999+0.5`` wins; fragments with that depth
  below 0 (in front of the near plane) are discarded, there is no far plane.
* Barycentrics are recomputed for the winning face at the pixel centre with the
  kernel's formula (alpha for tri[f,0], beta for tri[f,1], gamma for tri[f,2])
  and then perspective-corrected (b_i / w_i, renormalised).
* ``interpolate`` is the original pure-torch function, unchanged.

Backends
--------
``nvdiffrast`` (default for CUDA tensors): nvdiffrast's CUDA rasterizer
resolves visibility (one shared ``RasterizeCudaContext`` per device):
  * clip x/y are pre-scaled by (W-1)/W, (H-1)/H so nvdiffrast samples exactly the
    kernel's pixel-centre positions (no y flip needed: both use row 0 = NDC y=-1);
  * z is remapped z' = (z+w)/K - w when some z/w > 1 (same near plane and depth
    order, nothing clipped by nvdiffrast's far plane);
  * faces are submitted in reverse order (nvdiffrast keeps the later triangle on
    exact depth ties, the kernel the lowest index);
  * pixels that can disagree (non-uniform 3x3 neighbourhood or centre within
    0.25 px of its face's edges) are re-decided with the kernel's own rules
    (edge-inclusive float32 test, depth token, lowest index) among the faces in
    the 3x3 neighbourhood, plus nvdiffrast's second depth-peeling layer where the
    front face is rejected (an occluder edge can reveal the face behind it);
  * barycentrics are recomputed exactly as in ``barycentricFromImgcoordGPU``.
``torch``: an exact, vectorised port of the kernel (same float32 arithmetic,
same 2^18 depth quantisation, same "lowest face index wins" tie break via an
int64 scatter-amin).  Bit-identical to a literal per-face port of the CUDA code
in the tests; used for CPU tensors, ``use_depth_prior=1``, or when
``CUSTOM_RASTERIZER_BACKEND=torch`` / ``set_backend('torch')``.

Residual differences of the nvdiffrast backend (tests/test_raster_shim.py):
99.87-99.98 % identical face ids on adversarial low-res cases, every mismatch is
a pixel centre within 0.1 px of an edge (degenerate fans/slivers whose faces
nvdiffrast never draws near the pixel); barycentrics are bit-identical wherever
the face id matches; a full 6-view Hunyuan bake differs by 2e-4 mean.
"""

import os
import threading
import warnings

import torch

__all__ = ["rasterize", "interpolate", "get_backend", "set_backend", "rasterize_reference"]

_MAXINT = 2147483647  # MAXINT in rasterizer.h
_ZBUF_INIT = _MAXINT * _MAXINT + (_MAXINT - 1)  # "maxint" initial z-buffer token (fits in int64)
_DEPTH_SCALE = float(2 << 17)  # z_quantize = depth * (2<<17)
_OCCLUSION_TRUNCATION = 1e-6  # constant passed by custom_rasterizer/render.py

_backend = os.environ.get("CUSTOM_RASTERIZER_BACKEND", "nvdiffrast").strip().lower() or "nvdiffrast"
_ctx_lock = threading.Lock()
_ctx_cache = {}
_dr = None
_nvdiffrast_error = None


def set_backend(name):
    """Select 'nvdiffrast' (default) or 'torch' (exact kernel port)."""
    global _backend
    name = str(name).strip().lower()
    if name not in ("nvdiffrast", "torch"):
        raise ValueError("backend must be 'nvdiffrast' or 'torch'")
    _backend = name


def get_backend():
    return _backend


def _get_dr():
    global _dr, _nvdiffrast_error
    if _dr is None and _nvdiffrast_error is None:
        try:
            import nvdiffrast.torch as dr  # noqa: WPS433 (lazy import on purpose)
            _dr = dr
        except Exception as exc:  # pragma: no cover - depends on environment
            _nvdiffrast_error = exc
            warnings.warn(f"custom_rasterizer shim: nvdiffrast unavailable ({exc!r}); using exact torch backend")
    return _dr


def _get_context(device):
    """One RasterizeCudaContext per CUDA device, created once and reused."""
    dr = _get_dr()
    if dr is None:
        return None
    index = device.index if device.index is not None else torch.cuda.current_device()
    ctx = _ctx_cache.get(index)
    if ctx is None:
        with _ctx_lock:
            ctx = _ctx_cache.get(index)
            if ctx is None:
                ctx = dr.RasterizeCudaContext(device=torch.device("cuda", index))
                _ctx_cache[index] = ctx
    return ctx


# --------------------------------------------------------------------------------------
# Kernel arithmetic helpers (float32, same operation order as the CUDA source)
# --------------------------------------------------------------------------------------

def _screen_coords(V, width, height):
    """Per-vertex screen coordinates and depth exactly as rasterizeImagecoordsKernelGPU."""
    x, y, z, w = V[:, 0], V[:, 1], V[:, 2], V[:, 3]
    sx = (x / w * 0.5 + 0.5) * float(width - 1) + 0.5
    sy = (0.5 + 0.5 * y / w) * float(height - 1) + 0.5
    sz = z / w * 0.49999 + 0.5
    return sx, sy, sz


def _barycentric_2d(ax, ay, bx, by, cx, cy, px, py):
    """calculateBarycentricCoordinate(a, b, c, p) from rasterizer.h.

    calculateSignedArea2(a, b, c) = (c0-a0)*(b1-a1) - (b0-a0)*(c1-a1)
    beta = area2(a, p, c)/area, gamma = area2(a, b, p)/area, alpha = 1-beta-gamma;
    area == 0 -> (-1, -1, -1).  ``1.0 / area`` and ``1.0 - beta - gamma`` use double
    literals in the C++ source, so they are evaluated in float64 and rounded to float32.
    """
    beta_tri = (cx - ax) * (py - ay) - (px - ax) * (cy - ay)
    gamma_tri = (px - ax) * (by - ay) - (bx - ax) * (py - ay)
    area = (cx - ax) * (by - ay) - (bx - ax) * (cy - ay)
    degenerate = area == 0
    safe_area = torch.where(degenerate, torch.ones_like(area), area)
    tri_inv = (1.0 / safe_area.double()).float()
    beta = beta_tri * tri_inv
    gamma = gamma_tri * tri_inv
    alpha = (1.0 - beta.double() - gamma.double()).float()
    neg = torch.full_like(alpha, -1.0)
    alpha = torch.where(degenerate, neg, alpha)
    beta = torch.where(degenerate, neg, beta)
    gamma = torch.where(degenerate, neg, gamma)
    return alpha, beta, gamma


def _resolve_barycentrics(V, F, findices, width, height):
    """barycentricFromImgcoordGPU: recompute barycentrics of the winning face per pixel."""
    bary = torch.zeros((height, width, 3), dtype=torch.float32, device=V.device)
    covered = findices > 0
    if not bool(covered.any()):
        return bary
    flat = covered.reshape(-1).nonzero(as_tuple=False).squeeze(1)
    f = findices.reshape(-1)[flat].long() - 1
    tri = F[f]  # [M, 3]
    sx, sy, _ = _screen_coords(V, width, height)
    px = (flat % width).float() + 0.5
    py = torch.div(flat, width, rounding_mode="floor").float() + 0.5
    i0, i1, i2 = tri[:, 0], tri[:, 1], tri[:, 2]
    a, b, c = _barycentric_2d(sx[i0], sy[i0], sx[i1], sy[i1], sx[i2], sy[i2], px, py)
    a = a / V[i0, 3]
    b = b / V[i1, 3]
    c = c / V[i2, 3]
    s = 1.0 / (a + b + c)
    vals = torch.stack([a * s, b * s, c * s], dim=-1)
    bary.view(-1, 3)[flat] = vals
    return bary


# --------------------------------------------------------------------------------------
# Exact torch port of the rasterization kernel (pass 1 + pass 2)
# --------------------------------------------------------------------------------------

def _rasterize_torch(V, F, D, width, height, occlusion_truncation, use_depth_prior,
                     max_candidates=1 << 22):
    device = V.device
    V = V.to(torch.float32).contiguous()
    F = F.to(torch.int64).contiguous()
    num_faces = F.shape[0]
    zbuf = torch.full((height * width,), _ZBUF_INIT, dtype=torch.int64, device=device)
    if num_faces > 0 and width > 0 and height > 0:
        sx, sy, sz = _screen_coords(V, width, height)
        fx = sx[F]  # [F, 3]
        fy = sy[F]
        fz = sz[F]
        x_min, x_max = fx.min(dim=1).values, fx.max(dim=1).values
        y_min, y_max = fy.min(dim=1).values, fy.max(dim=1).values
        # conservative superset of the kernel loop bounds (coverage test is the real filter)
        x0 = torch.floor(x_min).clamp(min=0)
        x1 = (torch.floor(x_max) + 1).clamp(max=width - 1)
        y0 = torch.floor(y_min).clamp(min=0)
        y1 = (torch.floor(y_max) + 1).clamp(max=height - 1)
        finite = torch.isfinite(x0) & torch.isfinite(x1) & torch.isfinite(y0) & torch.isfinite(y1)
        x0 = torch.where(finite, x0, torch.zeros_like(x0)).long()
        x1 = torch.where(finite, x1, torch.full_like(x1, -1)).long()
        y0 = torch.where(finite, y0, torch.zeros_like(y0)).long()
        y1 = torch.where(finite, y1, torch.full_like(y1, -1)).long()
        nx = (x1 - x0 + 1).clamp(min=0)
        ny = (y1 - y0 + 1).clamp(min=0)
        counts = nx * ny
        cum_end = torch.cumsum(counts, 0)
        total = int(cum_end[-1].item()) if num_faces > 0 else 0
        cum_start = cum_end - counts
        prior = None
        if use_depth_prior:
            prior = D.reshape(-1).to(device=device, dtype=torch.float32)
        for start in range(0, total, max_candidates):
            ids = torch.arange(start, min(total, start + max_candidates), device=device, dtype=torch.int64)
            face = torch.searchsorted(cum_end, ids, right=True)
            local = ids - cum_start[face]
            nxf = nx[face]
            px_i = x0[face] + local % nxf
            py_i = y0[face] + torch.div(local, nxf, rounding_mode="floor")
            px = px_i.float() + 0.5
            py = py_i.float() + 0.5
            ax, bx, cx = fx[face, 0], fx[face, 1], fx[face, 2]
            ay, by, cy = fy[face, 0], fy[face, 1], fy[face, 2]
            alpha, beta, gamma = _barycentric_2d(ax, ay, bx, by, cx, cy, px, py)
            inside = (alpha >= 0) & (alpha <= 1) & (beta >= 0) & (beta <= 1) & (gamma >= 0) & (gamma <= 1)
            if not bool(inside.any()):
                continue
            face = face[inside]
            pixel = (py_i * width + px_i)[inside]
            alpha, beta, gamma = alpha[inside], beta[inside], gamma[inside]
            depth = alpha * fz[face, 0] + beta * fz[face, 1] + gamma * fz[face, 2]
            if prior is not None:
                depth_thres = prior[pixel] * 0.49999 + 0.5 + float(occlusion_truncation)
            else:
                depth_thres = torch.zeros_like(depth)
            z_quantize = (depth * _DEPTH_SCALE).to(torch.int64)  # C float->int truncation
            keep = ~(depth < depth_thres)
            if not bool(keep.any()):
                continue
            token = z_quantize[keep] * _MAXINT + (face[keep] + 1)
            zbuf.scatter_reduce_(0, pixel[keep], token, reduce="amin", include_self=True)
    f = torch.remainder(zbuf, _MAXINT)
    findices = torch.where(f == (_MAXINT - 1), torch.zeros_like(f), f).to(torch.int32).view(height, width)
    bary = _resolve_barycentrics(V, F, findices, width, height)
    return findices, bary


def rasterize_reference(pos, tri, resolution, clamp_depth=torch.zeros(0), use_depth_prior=0):
    """Same signature as ``rasterize`` but always uses the exact torch port."""
    V = pos[0]
    return _rasterize_torch(V, tri, clamp_depth, int(resolution[1]), int(resolution[0]),
                            _OCCLUSION_TRUNCATION, use_depth_prior)


# --------------------------------------------------------------------------------------
# nvdiffrast backend
# --------------------------------------------------------------------------------------

_EDGE_RETEST = os.environ.get("CUSTOM_RASTERIZER_EDGE_RETEST", "1") != "0"
# nvdiffrast's coverage differs from the kernel's float32 edge test by a few 1/100 px (fixed-point sub-pixel
# snapping + top-left rule); everything closer than this to its face's edges is re-decided with kernel rules.
_EDGE_MARGIN_PX = 0.25


def _kernel_tokens(F, sx, sy, sz, faces, px, py):
    """Kernel pass-1 decision for (face, pixel) pairs: z-buffer token if the face covers the pixel centre
    (edge-inclusive, depth >= 0), else the empty-buffer value."""
    tri = F[faces]
    i0, i1, i2 = tri[:, 0], tri[:, 1], tri[:, 2]
    a, b, c = _barycentric_2d(sx[i0], sy[i0], sx[i1], sy[i1], sx[i2], sy[i2],
                              px.float() + 0.5, py.float() + 0.5)
    inside = (a >= 0) & (a <= 1) & (b >= 0) & (b <= 1) & (c >= 0) & (c <= 1)
    depth = a * sz[i0] + b * sz[i1] + c * sz[i2]
    keep = inside & ~(depth < 0)
    z_quantize = torch.where(keep, depth * _DEPTH_SCALE, torch.zeros_like(depth)).to(torch.int64)
    token = z_quantize * _MAXINT + (faces + 1)
    return torch.where(keep, token, torch.full_like(token, _ZBUF_INIT))


def _edge_distance_px(F, sx, sy, faces, px, py):
    """Signed distance (pixels, >= 0 inside) from the pixel centre to the nearest edge of each face."""
    tri = F[faces]
    i0, i1, i2 = tri[:, 0], tri[:, 1], tri[:, 2]
    ax, ay, bx, by, cx, cy = sx[i0], sy[i0], sx[i1], sy[i1], sx[i2], sy[i2]
    a, b, c = _barycentric_2d(ax, ay, bx, by, cx, cy, px.float() + 0.5, py.float() + 0.5)
    area = ((cx - ax) * (by - ay) - (bx - ax) * (cy - ay)).abs()
    la = torch.hypot(cx - bx, cy - by).clamp_min(1e-12)
    lb = torch.hypot(ax - cx, ay - cy).clamp_min(1e-12)
    lc = torch.hypot(bx - ax, by - ay).clamp_min(1e-12)
    return torch.minimum(torch.minimum(a * area / la, b * area / lb), c * area / lc)


def _ambiguous_pixels(fid, F, sx, sy, width):
    """Flat indices of pixels nvdiffrast and the kernel may disagree on.

    * pixels whose 3x3 neighbourhood is not uniform (an edge of some visible face is within ~1 px; this also
      covers empty pixels next to covered ones, which the kernel's edge-inclusive test may cover), and
    * covered pixels whose centre is within _EDGE_MARGIN_PX of (or outside) their own face's edges in the
      kernel's float32 screen space.
    """
    covered = fid > 0
    fd = fid.to(torch.float64)[None, None]
    mx = torch.nn.functional.max_pool2d(fd, 3, 1, 1)[0, 0]
    mn = -torch.nn.functional.max_pool2d(-fd, 3, 1, 1)[0, 0]
    amb = (mx != fd[0, 0]) | (mn != fd[0, 0])
    flat_cov = covered.reshape(-1).nonzero(as_tuple=False).squeeze(1)
    if flat_cov.numel():
        f = fid.reshape(-1)[flat_cov] - 1
        dist = _edge_distance_px(F, sx, sy, f, flat_cov % width,
                                 torch.div(flat_cov, width, rounding_mode="floor"))
        amb.view(-1)[flat_cov[~(dist >= _EDGE_MARGIN_PX)]] = True
    return amb.reshape(-1).nonzero(as_tuple=False).squeeze(1)


def _neighbourhood_tokens(layer, idx, F, sx, sy, sz, width):
    """Kernel tokens of every face nvdiffrast put in the 3x3 neighbourhood of each pixel in ``idx``: [M, 9]."""
    px = idx % width
    py = torch.div(idx, width, rounding_mode="floor")
    padded = torch.nn.functional.pad(layer, (1, 1, 1, 1))
    cand = torch.stack([padded[py + dy, px + dx] for dy in range(3) for dx in range(3)], dim=1)
    tokens = torch.full(cand.shape, _ZBUF_INIT, dtype=torch.int64, device=layer.device)
    sel = (cand > 0).nonzero(as_tuple=False)
    if sel.numel():
        faces = cand[sel[:, 0], sel[:, 1]] - 1
        tokens[sel[:, 0], sel[:, 1]] = _kernel_tokens(F, sx, sy, sz, faces, px[sel[:, 0]], py[sel[:, 0]])
    return tokens


def _token_to_face(best):
    return torch.where(best < _ZBUF_INIT, torch.remainder(best, _MAXINT), torch.zeros_like(best))


def _rasterize_nvdiffrast(V, F, width, height):
    dr = _get_dr()
    ctx = _get_context(V.device) if dr is not None else None
    if ctx is None:
        return _rasterize_torch(V, F, None, width, height, _OCCLUSION_TRUNCATION, 0)
    V = V.to(torch.float32).contiguous()
    F_i32 = F.to(torch.int32).contiguous()
    if F_i32.shape[0] == 0 or width <= 0 or height <= 0:
        findices = torch.zeros((max(height, 0), max(width, 0)), dtype=torch.int32, device=V.device)
        return findices, torch.zeros((max(height, 0), max(width, 0), 3), dtype=torch.float32, device=V.device)
    x, y, z, w = V[:, 0], V[:, 1], V[:, 2], V[:, 3]
    # Kernel samples pixel px at NDC 2*px/(W-1)-1, nvdiffrast at (2*px+1)/W-1  ->  x_n = x_k*(W-1)/W
    xs = x * ((width - 1) / width)
    ys = y * ((height - 1) / height)
    # The kernel has no far plane (only depth < 0, i.e. z/w < -1, is rejected) while nvdiffrast clips
    # z/w > 1.  Remap z' = (z + w)/K - w (K >= 1): same near plane, same depth order, nothing beyond far.
    front = w > 0
    zs = z
    if bool(front.any()):
        zmax = float((z[front] / w[front]).max().item())
        if zmax > 1.0:
            k = (zmax + 1.0) * 0.5 * (1.0 + 1e-5) + 1e-6
            zs = (z + w) / k - w
    pos_n = torch.stack([xs, ys, zs, w], dim=-1).unsqueeze(0).contiguous()
    # nvdiffrast keeps the *later* triangle on exact depth ties, the kernel the lowest index: feed faces reversed
    num_faces = F_i32.shape[0]
    F_rev = torch.flip(F_i32, dims=[0]).contiguous()

    def to_original(rast):
        rev = torch.round(rast[0, ..., 3]).to(torch.int64)
        return torch.where(rev > 0, num_faces + 1 - rev, rev)

    F64 = F.to(torch.int64)
    resolution = [height, width]
    if not _EDGE_RETEST:
        rast, _ = dr.rasterize(ctx, pos_n, F_rev, resolution=resolution, grad_db=False)
        fid = to_original(rast)
    else:
        sx, sy, sz = _screen_coords(V, width, height)
        with dr.DepthPeeler(ctx, pos_n, F_rev, resolution=resolution, grad_db=False) as peeler:
            rast, _ = peeler.rasterize_next_layer()
            layer1 = to_original(rast)
            del rast
            fid = layer1
            idx = _ambiguous_pixels(layer1, F64, sx, sy, width)
            if idx.numel():
                tokens = _neighbourhood_tokens(layer1, idx, F64, sx, sy, sz, width)
                best = tokens.min(dim=1).values
                # front face rejected by the kernel's edge test -> the surface behind it may show through
                revealed = (layer1.reshape(-1)[idx] > 0) & (tokens[:, 4] == _ZBUF_INIT)
                if bool(revealed.any()):
                    rast, _ = peeler.rasterize_next_layer()
                    layer2 = to_original(rast)
                    del rast
                    ridx = idx[revealed]
                    tokens2 = _neighbourhood_tokens(layer2, ridx, F64, sx, sy, sz, width)
                    best[revealed] = torch.minimum(best[revealed], tokens2.min(dim=1).values)
                fid = layer1.clone()
                fid.view(-1)[idx] = _token_to_face(best)
    findices = fid.to(torch.int32)
    bary = _resolve_barycentrics(V, F64, findices, width, height)
    return findices, bary


# --------------------------------------------------------------------------------------
# Public API (same signatures as custom_rasterizer/render.py)
# --------------------------------------------------------------------------------------

def rasterize(pos, tri, resolution, clamp_depth=torch.zeros(0), use_depth_prior=0):
    assert (pos.device == tri.device)
    V = pos[0]
    height, width = int(resolution[0]), int(resolution[1])
    if use_depth_prior or V.device.type != "cuda" or _backend == "torch":
        return _rasterize_torch(V, tri, clamp_depth, width, height, _OCCLUSION_TRUNCATION, use_depth_prior)
    return _rasterize_nvdiffrast(V, tri, width, height)


def interpolate(col, findices, barycentric, tri):
    f = findices - 1 + (findices == 0)
    vcol = col[0, tri.long()[f.long()]]
    result = barycentric.view(*barycentric.shape, 1) * vcol
    result = torch.sum(result, axis=-2)
    return result.view(1, *result.shape)
