"""Validate the nvdiffrast-backed ``custom_rasterizer`` shim against the original kernel semantics.

Run with the embedded python (it ignores PYTHONPATH, paths are inserted here):
    E:\\ComfyUI\\ComfyUIStudio\\App\\python_embeded\\python.exe -s tools\\assetgen\\tests\\test_raster_shim.py

The reference below is written independently of the shim and follows
hy3dgen/texgen/custom_rasterizer/lib/custom_rasterizer_kernel/{rasterizer.h,rasterizer_gpu.cu}
line by line (per-face loop, kernel loop bounds, float32 arithmetic, edge-inclusive
barycentric test, 2^18 depth quantisation, atomicMin on (z_quantize*MAXINT + face+1),
pass-2 barycentric recomputation + perspective correction).  Functions are also
named test_* so the file works under pytest.
"""

import os
import sys
import time

import numpy as np
import torch

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETGEN = os.path.dirname(HERE)
SHIM_DIR = os.path.join(ASSETGEN, "shims")
if SHIM_DIR not in sys.path:
    sys.path.insert(0, SHIM_DIR)

import hy3d_bootstrap  # noqa: E402

PKG = hy3d_bootstrap.bootstrap()
import custom_rasterizer as cr  # noqa: E402  (must be the shim)

MAXINT = 2147483647
F32 = np.float32
RESULTS = []


def log(msg):
    print(msg, flush=True)


def check(name, ok, detail=""):
    RESULTS.append((name, bool(ok), detail))
    log(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    return ok


# --------------------------------------------------------------------------------------
# Literal reference of the CUDA kernel (CPU, float32)
# --------------------------------------------------------------------------------------

def _area2(a0, a1, b0, b1, c0, c1):
    # calculateSignedArea2(a, b, c) = (c0-a0)*(b1-a1) - (b0-a0)*(c1-a1)
    return (c0 - a0) * (b1 - a1) - (b0 - a0) * (c1 - a1)


def _bary(a, b, c, p0, p1):
    """calculateBarycentricCoordinate(a, b, c, p) with p a grid of points (torch float32)."""
    beta_tri = _area2(a[0], a[1], p0, p1, c[0], c[1])
    gamma_tri = _area2(a[0], a[1], b[0], b[1], p0, p1)
    area = _area2(a[0], a[1], b[0], b[1], c[0], c[1])
    if float(area) == 0.0:
        neg = torch.full_like(p0, -1.0)
        return neg, neg.clone(), neg.clone()
    tri_inv = torch.tensor(1.0 / float(area), dtype=torch.float32)  # 1.0 / area: double, stored in float
    beta = beta_tri * tri_inv
    gamma = gamma_tri * tri_inv
    alpha = (1.0 - beta.double() - gamma.double()).float()  # 1.0 - beta - gamma in double
    return alpha, beta, gamma


def reference_rasterize(pos, tri, resolution, D=None, use_depth_prior=0, occlusion_truncation=1e-6):
    V = pos[0].detach().to("cpu", torch.float32)
    Fc = tri.detach().to("cpu", torch.int64)
    height, width = int(resolution[0]), int(resolution[1])  # rasterize_image(..., resolution[1], resolution[0])
    zbuf = torch.full((height * width,), MAXINT * MAXINT + (MAXINT - 1), dtype=torch.int64)
    x, y, z, w = V[:, 0], V[:, 1], V[:, 2], V[:, 3]
    sx = (x / w * 0.5 + 0.5) * float(width - 1) + 0.5
    sy = (0.5 + 0.5 * y / w) * float(height - 1) + 0.5
    sz = z / w * 0.49999 + 0.5
    Dflat = None if D is None or not use_depth_prior else D.detach().to("cpu", torch.float32).reshape(-1)
    for f in range(Fc.shape[0]):
        i0, i1, i2 = (int(v) for v in Fc[f])
        a = (sx[i0], sy[i0]); b = (sx[i1], sy[i1]); c = (sx[i2], sy[i2])
        xs = [F32(sx[i].item()) for i in (i0, i1, i2)]
        ys = [F32(sy[i].item()) for i in (i0, i1, i2)]
        if not all(np.isfinite(xs + ys)):
            continue
        x_min, x_max = min(xs), max(xs)
        y_min, y_max = min(ys), max(ys)
        # for (int px = x_min; px < x_max + 1; ++px) { if (px < 0 || px >= width) continue; ...
        px_list = []
        px = int(x_min)  # C float->int conversion truncates toward zero
        while F32(px) < F32(x_max + F32(1)):
            if 0 <= px < width:
                px_list.append(px)
            px += 1
        py_list = []
        py = int(y_min)
        while F32(py) < F32(y_max + F32(1)):
            if 0 <= py < height:
                py_list.append(py)
            py += 1
        if not px_list or not py_list:
            continue
        gy, gx = torch.meshgrid(torch.tensor(py_list), torch.tensor(px_list), indexing="ij")
        gx = gx.reshape(-1); gy = gy.reshape(-1)
        p0 = gx.float() + 0.5
        p1 = gy.float() + 0.5
        al, be, ga = _bary(a, b, c, p0, p1)
        inside = (al >= 0) & (al <= 1) & (be >= 0) & (be <= 1) & (ga >= 0) & (ga <= 1)
        if not bool(inside.any()):
            continue
        pix = (gy * width + gx)[inside]
        depth = al[inside] * sz[i0] + be[inside] * sz[i1] + ga[inside] * sz[i2]
        if Dflat is not None:
            thres = Dflat[pix] * 0.49999 + 0.5 + occlusion_truncation
        else:
            thres = torch.zeros_like(depth)
        zq = (depth * float(2 << 17)).to(torch.int64)
        token = zq * MAXINT + (f + 1)
        keep = ~(depth < thres)
        for p_, t_ in zip(pix[keep].tolist(), token[keep].tolist()):
            if t_ < zbuf[p_]:
                zbuf[p_] = t_
    fz = torch.remainder(zbuf, MAXINT)
    findices = torch.where(fz == MAXINT - 1, torch.zeros_like(fz), fz).to(torch.int32).view(height, width)
    bary = torch.zeros(height, width, 3, dtype=torch.float32)
    covered = (findices > 0).nonzero(as_tuple=False)
    for py, px in covered.tolist():
        f = int(findices[py, px]) - 1
        i0, i1, i2 = (int(v) for v in Fc[f])
        p0 = torch.tensor([float(px) + 0.5], dtype=torch.float32)
        p1 = torch.tensor([float(py) + 0.5], dtype=torch.float32)
        al, be, ga = _bary((sx[i0], sy[i0]), (sx[i1], sy[i1]), (sx[i2], sy[i2]), p0, p1)
        al = al / w[i0]; be = be / w[i1]; ga = ga / w[i2]
        s = 1.0 / (al + be + ga)
        bary[py, px, 0] = (al * s)[0]
        bary[py, px, 1] = (be * s)[0]
        bary[py, px, 2] = (ga * s)[0]
    return findices, bary


# --------------------------------------------------------------------------------------
# Helpers
# --------------------------------------------------------------------------------------

def edge_distance_px(pos, tri, resolution, findices_face, ys, xs):
    """Signed pixel distance from pixel centres to the nearest edge of the given faces (kernel screen space)."""
    V = pos[0].detach().to("cpu", torch.float64)
    height, width = int(resolution[0]), int(resolution[1])
    sx = (V[:, 0] / V[:, 3] * 0.5 + 0.5) * (width - 1) + 0.5
    sy = (0.5 + 0.5 * V[:, 1] / V[:, 3]) * (height - 1) + 0.5
    T = tri.detach().to("cpu", torch.int64)[findices_face]
    ax, ay = sx[T[:, 0]], sy[T[:, 0]]
    bx, by = sx[T[:, 1]], sy[T[:, 1]]
    cx, cy = sx[T[:, 2]], sy[T[:, 2]]
    px = xs.double() + 0.5; py = ys.double() + 0.5
    area = (cx - ax) * (by - ay) - (bx - ax) * (cy - ay)
    sgn = torch.sign(area)
    d = []
    for (ux, uy, vx, vy) in ((bx, by, cx, cy), (cx, cy, ax, ay), (ax, ay, bx, by)):
        cross = (vx - ux) * (py - uy) - (px - ux) * (vy - uy)
        length = torch.sqrt((vx - ux) ** 2 + (vy - uy) ** 2).clamp(min=1e-12)
        d.append(-sgn * cross / length)
    return torch.stack(d, -1).min(-1).values  # >= 0 inside, < 0 outside


def quantized_depth(pos, tri, resolution, face, ys, xs):
    V = pos[0].detach().to("cpu", torch.float32)
    height, width = int(resolution[0]), int(resolution[1])
    x, y, z, w = V[:, 0], V[:, 1], V[:, 2], V[:, 3]
    sx = (x / w * 0.5 + 0.5) * float(width - 1) + 0.5
    sy = (0.5 + 0.5 * y / w) * float(height - 1) + 0.5
    sz = z / w * 0.49999 + 0.5
    out = []
    T = tri.detach().to("cpu", torch.int64)
    for f, py, px in zip(face.tolist(), ys.tolist(), xs.tolist()):
        i0, i1, i2 = T[f].tolist()
        al, be, ga = _bary((sx[i0], sy[i0]), (sx[i1], sy[i1]), (sx[i2], sy[i2]),
                           torch.tensor([px + 0.5]), torch.tensor([py + 0.5]))
        dep = al * sz[i0] + be * sz[i1] + ga * sz[i2]
        out.append(int((dep * float(2 << 17)).to(torch.int64)[0]))
    return torch.tensor(out)


def compare(name, pos, tri, resolution, ref, got, exact, edge_tol_px=0.1, min_match=0.995, raw=None):
    """raw: nvdiffrast first-layer face ids without the kernel re-test (used to explain residual mismatches)."""
    rf, rb = ref
    gf, gb = (got[0].cpu(), got[1].cpu())
    ok = True
    ok &= check(f"{name}: findices dtype/shape", gf.dtype == torch.int32 and tuple(gf.shape) == tuple(rf.shape)
                and gb.dtype == torch.float32 and tuple(gb.shape) == tuple(rb.shape),
                f"{gf.dtype} {tuple(gf.shape)} / {gb.dtype} {tuple(gb.shape)}")
    same = gf == rf
    frac = same.float().mean().item()
    covered = int((rf > 0).sum())
    agree_cov = same & (rf > 0)
    bdiff = (gb - rb).abs()
    max_b = bdiff[agree_cov].max().item() if bool(agree_cov.any()) else 0.0
    bg_zero = bool((gb[gf == 0] == 0).all())
    if exact:
        ok &= check(f"{name}: face ids identical", bool(same.all()), f"{int((~same).sum())} mismatches / {rf.numel()}")
        ok &= check(f"{name}: barycentrics identical", bool(torch.equal(gb, rb)), f"max|diff|={bdiff.max().item():.3g}")
        return ok
    mism = (~same).nonzero(as_tuple=False)
    unexplained = 0
    n_edge = n_tie = 0
    if mism.numel():
        ys, xs = mism[:, 0], mism[:, 1]
        rfa, gfa = rf[ys, xs].long(), gf[ys, xs].long()
        near_edge = torch.zeros(len(ys), dtype=torch.bool)
        # a mismatch is "edge precision" if the pixel centre is within edge_tol_px of an edge of the reference
        # face, the returned face, or the face nvdiffrast drew in front (whose edge decides what shows through)
        faces_to_check = [rfa, gfa]
        if raw is not None:
            faces_to_check.append(raw.cpu()[ys, xs].long())
        for face_vec in faces_to_check:
            sel = face_vec > 0
            if bool(sel.any()):
                d = edge_distance_px(pos, tri, resolution, face_vec[sel] - 1, ys[sel], xs[sel])
                near_edge[sel] |= d.abs() <= edge_tol_px
        tie = torch.zeros(len(ys), dtype=torch.bool)
        both = (rfa > 0) & (gfa > 0)
        if bool(both.any()):
            qr = quantized_depth(pos, tri, resolution, rfa[both] - 1, ys[both], xs[both])
            qg = quantized_depth(pos, tri, resolution, gfa[both] - 1, ys[both], xs[both])
            tie[both] = (qr - qg).abs() <= 1
        explained = near_edge | tie
        n_tie = int((tie & ~near_edge).sum())
        n_edge = int(near_edge.sum())
        unexplained = int((~explained).sum())
        if unexplained:
            bad = (~explained).nonzero().squeeze(1)[:5]
            log(f"      unexplained pixels (y, x, ref, got): "
                f"{[(int(ys[i]), int(xs[i]), int(rfa[i]), int(gfa[i])) for i in bad]}")
    detail = (f"match={frac * 100:.3f}% ({int((~same).sum())} px differ of {rf.numel()}, covered={covered}); "
              f"explained: edge(<= {edge_tol_px}px)={n_edge} depth-tie={n_tie}; unexplained={unexplained}; "
              f"max|bary diff| on agreeing px={max_b:.3g}")
    ok &= check(f"{name}: face-id agreement", frac >= min_match and unexplained == 0, detail)
    ok &= check(f"{name}: barycentrics match where ids agree", max_b <= 1e-5, f"max={max_b:.3g}")
    ok &= check(f"{name}: background barycentrics are zero", bg_zero)
    return ok


def run_backends(pos, tri, resolution, D=None, use_depth_prior=0):
    out = {}
    clamp = D if D is not None else torch.zeros(0)
    cr.set_backend("nvdiffrast")
    out["nvdiffrast"] = cr.rasterize(pos, tri, resolution, clamp, use_depth_prior)
    cr._EDGE_RETEST = False
    out["nvdiffrast_raw"] = cr.rasterize(pos, tri, resolution, clamp, use_depth_prior)
    cr._EDGE_RETEST = True
    cr.set_backend("torch")
    out["torch"] = cr.rasterize(pos, tri, resolution, clamp, use_depth_prior)
    cr.set_backend("nvdiffrast")
    return out


def raw_agreement(ref_f, raw):
    return (raw[0].cpu() == ref_f).float().mean().item()


# --------------------------------------------------------------------------------------
# Tests
# --------------------------------------------------------------------------------------

def test_shim_is_used():
    ok = check("custom_rasterizer is the shim", os.path.normcase(cr.__file__).startswith(os.path.normcase(SHIM_DIR)),
               cr.__file__)
    import nvdiffrast.torch as dr  # noqa: F401
    cr.set_backend("nvdiffrast")
    ctx1 = cr._get_context(torch.device("cuda"))
    ctx2 = cr._get_context(torch.device("cuda", 0))
    ok &= check("single RasterizeCudaContext reused", ctx1 is ctx2 and len(cr._ctx_cache) == 1)
    return ok


def test_analytic_conventions():
    """Hand-checked kernel conventions on an 11x13 image (H=11, W=13)."""
    dev = "cuda"
    H, W = 11, 13
    # NDC corners map to pixel *centres*: x=-1 -> sx=0.5 (px 0), x=+1 -> sx=W-0.5 (px W-1); y=-1 -> row 0
    pos = torch.tensor([[[-1.0, -1.0, 0.0, 1.0], [1.0, -1.0, 0.0, 1.0], [-1.0, 1.0, 0.0, 1.0]]], device=dev)
    tri = torch.tensor([[0, 1, 2]], dtype=torch.int32, device=dev)
    ok = True
    for backend in ("torch", "nvdiffrast"):
        cr.set_backend(backend)
        f, b = cr.rasterize(pos, tri, (H, W))
        f, b = f.cpu(), b.cpu()
        # inside iff px/(W-1) + py/(H-1) <= 1
        yy, xx = torch.meshgrid(torch.arange(H), torch.arange(W), indexing="ij")
        lhs = xx.double() / (W - 1) + yy.double() / (H - 1)
        strict_inside = lhs < 1 - 1e-9
        on_edge = (lhs - 1).abs() <= 1e-9
        ok &= check(f"analytic[{backend}]: resolution order (H, W) -> shape {tuple(f.shape)}", tuple(f.shape) == (H, W))
        ok &= check(f"analytic[{backend}]: strict interior covered (face id 1)", bool((f[strict_inside] == 1).all()))
        ok &= check(f"analytic[{backend}]: outside empty", bool((f[(lhs > 1 + 1e-9)] == 0).all()))
        n_edge_cov = int((f[on_edge] == 1).sum())
        if backend == "torch":
            ok &= check("analytic[torch]: edge-inclusive coverage (kernel semantics)", n_edge_cov == int(on_edge.sum()),
                        f"{n_edge_cov}/{int(on_edge.sum())} hypotenuse pixels")
        else:
            log(f"      note: nvdiffrast covers {n_edge_cov}/{int(on_edge.sum())} pixels whose centre lies exactly on "
                f"the hypotenuse (kernel: all; nvdiffrast uses a top-left fill rule)")
        # row 0 = NDC y=-1 (bottom edge of the triangle), column 0 = NDC x=-1
        ok &= check(f"analytic[{backend}]: pixel (row0,col0) is vertex 0 -> bary (1,0,0)",
                    torch.allclose(b[0, 0], torch.tensor([1.0, 0.0, 0.0]), atol=1e-6), str(b[0, 0].tolist()))
        if f[0, W - 1] == 1:
            ok &= check(f"analytic[{backend}]: (row0,col W-1) is vertex 1 -> (0,1,0)",
                        torch.allclose(b[0, W - 1], torch.tensor([0.0, 1.0, 0.0]), atol=1e-6), str(b[0, W - 1].tolist()))
        if f[H - 1, 0] == 1:
            ok &= check(f"analytic[{backend}]: (row H-1,col0) is vertex 2 -> (0,0,1)",
                        torch.allclose(b[H - 1, 0], torch.tensor([0.0, 0.0, 1.0]), atol=1e-6), str(b[H - 1, 0].tolist()))
        py, px = 3, 4
        exp = torch.tensor([1 - px / (W - 1) - py / (H - 1), px / (W - 1), py / (H - 1)], dtype=torch.float32)
        ok &= check(f"analytic[{backend}]: bary at (row3,col4) = (1-u-v, u, v) with u=px/(W-1), v=py/(H-1)",
                    torch.allclose(b[py, px], exp, atol=1e-6), f"{b[py, px].tolist()} vs {exp.tolist()}")
        ok &= check(f"analytic[{backend}]: background barycentric = 0", bool((b[f == 0] == 0).all()))
    cr.set_backend("nvdiffrast")
    return ok


def test_depth_and_clipping_semantics():
    """Nearest z/w wins, ties -> lowest face index, near rejection, no far plane, no culling, degenerate faces."""
    dev = "cuda"
    H, W = 16, 16
    big = [[-1.0, -1.0], [3.0, -1.0], [-1.0, 3.0]]

    def tri_at(z, cw=False):
        pts = big if not cw else [big[0], big[2], big[1]]
        return [[x, y, z, 1.0] for x, y in pts]

    ok = True
    cases = {
        "nearest wins (z=-0.5 vs 0.5)": ([tri_at(0.5), tri_at(-0.5)], 2),
        "exact tie -> lowest index": ([tri_at(0.25), tri_at(0.25)], 1),
        "beyond far plane still drawn (z=1.5)": ([tri_at(1.5)], 1),
        "beyond far plane, nearest of two (1.5 vs 3.0)": ([tri_at(3.0), tri_at(1.5)], 2),
        "in front of near plane rejected (z=-1.5)": ([tri_at(-1.5)], 0),
        "clockwise (back-facing) drawn": ([tri_at(0.0, cw=True)], 1),
        "degenerate (collinear) never drawn": ([[[-1, -1, 0, 1], [0, 0, 0, 1], [1, 1, 0, 1]]], 0),
    }
    for label, (tris, expected) in cases.items():
        verts = torch.tensor([v for t in tris for v in t], dtype=torch.float32, device=dev)[None]
        faces = torch.arange(verts.shape[1], device=dev, dtype=torch.int32).view(-1, 3)
        ref_f, _ = reference_rasterize(verts, faces, (H, W))
        res = run_backends(verts, faces, (H, W))
        res.pop("nvdiffrast_raw")
        vals = {k: torch.unique(v[0].cpu()).tolist() for k, v in res.items()}
        ref_vals = torch.unique(ref_f).tolist()
        good = ref_vals == [expected] and all(v == [expected] for v in vals.values())
        ok &= check(f"semantics: {label}", good, f"reference={ref_vals} shim={vals}")
    return ok


def _random_soup(n, seed, perspective, dev="cuda"):
    g = torch.Generator().manual_seed(seed)
    centers = torch.rand(n, 1, 2, generator=g) * 2.6 - 1.3  # some partially off-screen
    sizes = torch.rand(n, 1, 1, generator=g) * 0.6 + 0.02
    xy = centers + (torch.rand(n, 3, 2, generator=g) - 0.5) * 2 * sizes
    z = torch.rand(n, 1, 1, generator=g).expand(n, 3, 1) * 1.6 - 0.8 + (torch.rand(n, 3, 1, generator=g) - 0.5) * 0.3
    z[: max(1, n // 20)] += 1.2  # a few beyond the far plane (z/w > 1): the kernel still draws them
    w = torch.ones(n, 3, 1)
    if perspective:
        w = torch.rand(n, 3, 1, generator=g) * 1.5 + 0.5
    v = torch.cat([xy * w, z * w, w], dim=-1).reshape(1, -1, 4).to(dev)
    f = torch.arange(n * 3, dtype=torch.int32).view(n, 3).to(dev)  # independent triangles (shared edges: mesh tests)
    return v.contiguous(), f.contiguous()


def test_random_soups():
    ok = True
    for (n, seed, persp, res) in ((150, 1, False, (37, 53)), (150, 2, True, (64, 48)), (300, 3, True, (96, 128))):
        pos, tri = _random_soup(n, seed, persp)
        t0 = time.time()
        ref = reference_rasterize(pos, tri, res)
        t_ref = time.time() - t0
        got = run_backends(pos, tri, res)
        name = f"soup n={n} {'persp' if persp else 'ortho'} {res[0]}x{res[1]}"
        ok &= compare(name + " [torch port]", pos, tri, res, ref, got["torch"], exact=True)
        ok &= compare(name + " [nvdiffrast]", pos, tri, res, ref, got["nvdiffrast"], exact=False, min_match=0.99,
                      raw=got["nvdiffrast_raw"][0])
        log(f"      (reference took {t_ref:.2f}s; plain nvdiffrast without kernel re-test would match "
            f"{raw_agreement(ref[0], got['nvdiffrast_raw']) * 100:.3f}%)")
    return ok


def _hunyuan_camera_views(mesh_vertices, faces, views, resolution):
    """Render clip-space positions exactly like MeshRender.get_pos_from_mvp (orthographic camera)."""
    cam = hy3d_bootstrap.hy3d_import("hy3dgen.texgen.differentiable_renderer.camera_utils")
    proj = cam.get_orthographic_projection_matrix(left=-0.6, right=0.6, bottom=-0.6, top=0.6, near=0.1, far=100)
    vtx = torch.as_tensor(mesh_vertices, dtype=torch.float32, device="cuda")
    # MeshRender.set_mesh: (x, y, z) -> (-x, z, -y), then auto-centre/scale to 1.15
    vtx[:, [0, 1]] = -vtx[:, [0, 1]]
    vtx[:, [1, 2]] = vtx[:, [2, 1]]
    center = (vtx.max(0)[0] + vtx.min(0)[0]) / 2
    scale = torch.norm(vtx - center, dim=1).max() * 2.0
    vtx = (vtx - center) * (1.15 / float(scale))
    out = []
    for elev, azim in views:
        mv = cam.get_mv_matrix(elev=elev, azim=azim, camera_distance=1.45, center=None)
        pos_camera = cam.transform_pos(mv, vtx, keepdim=True)
        pos_clip = cam.transform_pos(proj, pos_camera)
        out.append((elev, azim, pos_clip))
    tri = torch.as_tensor(np.asarray(faces), dtype=torch.int32, device="cuda")
    return vtx, tri, out


def test_mesh_views_against_reference():
    import trimesh
    mesh = trimesh.creation.icosphere(subdivisions=3)
    mesh.vertices = mesh.vertices * np.array([1.0, 0.7, 0.45])  # ellipsoid
    knob = trimesh.creation.box(extents=(0.5, 0.25, 0.25))
    knob.apply_translation((0.9, 0.2, 0.0))
    mesh = trimesh.util.concatenate([mesh, knob])  # interpenetrating parts -> real occlusion
    vtx, tri, views = _hunyuan_camera_views(mesh.vertices, mesh.faces, [(0, 0), (0, 90), (90, 0), (-20, 210)], 96)
    ok = True
    res = (96, 96)
    for elev, azim, pos in views:
        ref = reference_rasterize(pos, tri, res)
        got = run_backends(pos, tri, res)
        name = f"ellipsoid+box elev={elev} azim={azim} {res[0]}x{res[1]}"
        ok &= compare(name + " [torch port]", pos, tri, res, ref, got["torch"], exact=True)
        ok &= compare(name + " [nvdiffrast]", pos, tri, res, ref, got["nvdiffrast"], exact=False, min_match=0.995,
                      raw=got["nvdiffrast_raw"][0])
        log(f"      (plain nvdiffrast without kernel re-test would match "
            f"{raw_agreement(ref[0], got['nvdiffrast_raw']) * 100:.3f}%)")
    # orientation: MeshRender maps mesh (x, y, z) -> (-x, z, -y) and the camera's up is +Z, so mesh +y is
    # camera -y; with the kernel convention (row 0 = NDC y=-1) the mesh top lands in the *first* rows, i.e. the
    # array is upright when saved as an image.  A y-flipped rasterizer would put it in the last rows.
    elev, azim, pos = views[0]
    f, _ = cr.rasterize(pos, tri, (128, 128))
    rows = (f > 0).any(dim=1).nonzero().squeeze(1)
    upper_face = int(np.argmax(mesh.triangles_center[:, 1]))  # topmost face in mesh space
    hit_rows = (f == upper_face + 1).any(dim=1).nonzero().squeeze(1)
    ok &= check("orientation: mesh top appears in the first rows (row 0 = NDC y=-1, saved image upright)",
                hit_rows.numel() > 0 and hit_rows.float().mean() < rows.float().mean(),
                f"top-face rows {hit_rows.tolist()[:4]} vs coverage rows {int(rows.min())}..{int(rows.max())}")
    return ok


def test_depth_prior_path():
    pos, tri = _random_soup(120, 7, False)
    res = (40, 40)
    # depth prior = z/w of the nearest surface + 0 -> peel to the 2nd layer
    first_f, first_b = cr.rasterize_reference(pos, tri, res)
    zw = (pos[0, :, 2] / pos[0, :, 3])
    prior = cr.interpolate(zw.view(1, -1, 1), first_f, first_b, tri)[0, ..., 0]
    prior = torch.where(first_f > 0, prior, torch.full_like(prior, -2.0)).contiguous()
    ref = reference_rasterize(pos, tri, res, D=prior, use_depth_prior=1)
    got = cr.rasterize(pos, tri, res, prior, 1)
    return compare("depth-prior (use_depth_prior=1, peeling) [torch port]", pos, tri, res, ref, got, exact=True)


def test_vendored_meshrender_integration():
    """Run the vendored MeshRender (render_normal/position, back_project, uv_feature_map, uv_inpaint)."""
    import trimesh
    mr = hy3d_bootstrap.hy3d_import("hy3dgen.texgen.differentiable_renderer.mesh_render")
    mp = sys.modules[f"{PKG}.hy3dgen.texgen.differentiable_renderer.mesh_processor"]
    uvw = hy3d_bootstrap.hy3d_import("hy3dgen.texgen.utils.uv_warp_utils")
    ok = check("mesh_processor resolves to the pure-python port (cp312 .pyd skipped on py3.11)",
               mp.__file__.lower().endswith("mesh_processor.py"), mp.__file__)
    mesh = trimesh.creation.capsule(height=0.8, radius=0.35, count=[24, 24])
    mesh = uvw.mesh_uv_wrap(mesh)
    outs = {}
    for backend in ("torch", "nvdiffrast"):
        cr.set_backend(backend)
        r = mr.MeshRender(default_resolution=256, texture_size=256)
        r.load_mesh(mesh)
        normal, mask = r.render_normal(0, 0, use_abs_coor=True, return_type="th")
        position = r.render_position(90, 0, return_type="th")
        img = torch.rand(256, 256, 3, generator=torch.Generator().manual_seed(0))
        tex, cos, bnd = r.back_project(img, 0, 90)
        uvpos = r.render_uvpos(return_type="th")
        outs[backend] = (normal.cpu(), mask.cpu(), position.cpu(), tex.cpu(), cos.cpu(), uvpos.cpu(), r)
    cr.set_backend("nvdiffrast")
    names = ["render_normal", "visible mask", "render_position", "back_project texture", "back_project cos map",
             "uv_feature_map(render_uvpos)"]
    # back_project normalises depth by the min/max over *visible* pixels and runs Canny on it, so one
    # silhouette pixel that changes face can rescale the whole depth image and move the "unreliable" mask;
    # it also accumulates with CUDA scatter_add_ (atomics: not bit-reproducible even run-to-run).  Those two
    # outputs are reported, not asserted; the raster inputs they are built from are asserted above/below.
    informational = {"back_project texture", "back_project cos map"}
    for i, nm in enumerate(names):
        a, b = outs["torch"][i].float(), outs["nvdiffrast"][i].float()
        diff = (a - b).abs()
        frac_bad = (diff.reshape(-1) > 1e-3).float().mean().item()
        detail = f"{frac_bad * 100:.3f}% values differ >1e-3, max diff {diff.max().item():.3g}"
        if nm in informational:
            log(f"[INFO] MeshRender {nm}: nvdiffrast vs exact port -- {detail} (amplified by depth min/max "
                f"normalisation + Canny; see comment)")
        else:
            ok &= check(f"MeshRender {nm}: nvdiffrast vs exact port", frac_bad < 0.01, detail)
    # meshVerticeInpaint: vendored pure-python port vs numba port of mesh_processor.cpp
    import mesh_processor_fast as mpf
    r = outs["nvdiffrast"][-1]
    tex = outs["nvdiffrast"][3]
    mask = (outs["nvdiffrast"][4].squeeze(-1) > 0).numpy().astype(np.uint8) * 255
    vtx_pos, pos_idx, vtx_uv, uv_idx = r.get_mesh()
    t0 = time.time()
    ref_t, ref_m = mp.meshVerticeInpaint(tex.numpy(), mask, vtx_pos, vtx_uv, pos_idx, uv_idx)
    t_py = time.time() - t0
    t0 = time.time()
    mpf.meshVerticeInpaint(tex.numpy(), mask, vtx_pos, vtx_uv, pos_idx, uv_idx)  # includes JIT / cache load
    t_first = time.time() - t0
    t0 = time.time()
    fast_t, fast_m = mpf.meshVerticeInpaint(tex.numpy(), mask, vtx_pos, vtx_uv, pos_idx, uv_idx)
    t_fast = time.time() - t0
    dmax = float(np.abs(fast_t.astype(np.float64) - ref_t.astype(np.float64)).max())
    ok &= check("meshVerticeInpaint numba port == vendored .py", dmax < 1e-4 and np.array_equal(fast_m, ref_m),
                f"max|diff|={dmax:.2g}, masks equal={np.array_equal(fast_m, ref_m)}; .py {t_py:.2f}s, "
                f"numba {t_fast * 1000:.1f} ms (first call {t_first:.2f}s), {len(vtx_pos)} verts")
    mpf.install(mr)
    t0 = time.time()
    painted = r.uv_inpaint(tex, mask)
    ok &= check("MeshRender.uv_inpaint (fast meshVerticeInpaint + cv2.inpaint) runs",
                painted.shape == (256, 256, 3) and painted.dtype == np.uint8, f"{(time.time() - t0) * 1000:.0f} ms")
    return ok


def test_full_bake_equivalence():
    """Hunyuan's 6-view bake (back_project x6 + fast_bake_texture + uv_inpaint) with smooth synthetic views:
    final textures from the nvdiffrast backend vs the exact kernel port."""
    import trimesh
    import mesh_processor_fast as mpf
    mr = hy3d_bootstrap.hy3d_import("hy3dgen.texgen.differentiable_renderer.mesh_render")
    uvw = hy3d_bootstrap.hy3d_import("hy3dgen.texgen.utils.uv_warp_utils")
    mpf.install(mr)
    mesh = trimesh.creation.icosphere(subdivisions=4)
    mesh.vertices = mesh.vertices * np.array([1.0, 0.75, 0.5])
    mesh = uvw.mesh_uv_wrap(mesh)
    elevs, azims = [0, 0, 0, 0, 90, -90], [0, 90, 180, 270, 0, 180]
    weights = [1, 0.1, 0.5, 0.1, 0.05, 0.05]
    results = {}
    for backend in ("torch", "nvdiffrast"):
        cr.set_backend(backend)
        r = mr.MeshRender(default_resolution=512, texture_size=512)
        r.load_mesh(mesh)
        views = [r.render_position(e, a, return_type="pl") for e, a in zip(elevs, azims)]
        textures, coss = [], []
        for v, e, a, w in zip(views, elevs, azims, weights):
            t, c, _ = r.back_project(v, e, a)
            textures.append(t)
            coss.append(w * c ** 4)
        tex, trust = r.fast_bake_texture(textures, coss)
        mask = ((trust > 1e-8).squeeze(-1).cpu().numpy() * 255).astype(np.uint8)
        results[backend] = (r.uv_inpaint(tex, mask).astype(np.float32) / 255.0, mask)
    cr.set_backend("nvdiffrast")
    (ta, ma), (tb, mb) = results["torch"], results["nvdiffrast"]
    diff = np.abs(ta - tb).max(axis=-1)
    covered = (ma > 0) | (mb > 0)
    mean_d = float(diff[covered].mean())
    frac_big = float((diff[covered] > 0.1).mean())
    mask_agree = float((ma == mb).mean())
    return check("full 6-view bake + inpaint: nvdiffrast vs exact port (512 px, 5120-face ellipsoid)",
                 mean_d < 0.01 and frac_big < 0.01,
                 f"mean |diff| {mean_d:.4f}, texels with |diff|>0.1: {frac_big * 100:.3f}%, "
                 f"trust-mask agreement {mask_agree * 100:.3f}%")


def test_performance():
    import trimesh
    mesh = trimesh.creation.icosphere(subdivisions=6)  # 81920 faces
    vtx, tri, views = _hunyuan_camera_views(mesh.vertices, mesh.faces, [(0, 0)], 2048)
    pos = views[0][2]
    timings = {}
    for backend in ("nvdiffrast", "torch"):
        cr.set_backend(backend)
        cr.rasterize(pos, tri, (2048, 2048))  # warm-up
        torch.cuda.synchronize()
        t0 = time.time()
        for _ in range(3):
            f, b = cr.rasterize(pos, tri, (2048, 2048))
        torch.cuda.synchronize()
        timings[backend] = (time.time() - t0) / 3
        if backend == "nvdiffrast":
            f_n = f
        else:
            f_t = f
    cr.set_backend("nvdiffrast")
    agree = (f_n == f_t).float().mean().item()
    return check("performance 2048x2048, 81920 faces", agree > 0.999,
                 f"nvdiffrast {timings['nvdiffrast'] * 1000:.1f} ms, exact torch port {timings['torch'] * 1000:.1f} ms, "
                 f"face-id agreement {agree * 100:.4f}%")


def main():
    torch.backends.cuda.matmul.allow_tf32 = False
    tests = [test_shim_is_used, test_analytic_conventions, test_depth_and_clipping_semantics, test_random_soups,
             test_mesh_views_against_reference, test_depth_prior_path, test_vendored_meshrender_integration,
             test_full_bake_equivalence, test_performance]
    for t in tests:
        log(f"--- {t.__name__}")
        try:
            t()
        except Exception as exc:  # report and continue
            import traceback
            traceback.print_exc()
            check(f"{t.__name__} raised", False, repr(exc))
    n_fail = sum(1 for _, ok, _ in RESULTS if not ok)
    log(f"=== {len(RESULTS) - n_fail}/{len(RESULTS)} checks passed")
    return 1 if n_fail else 0


if __name__ == "__main__":
    sys.exit(main())
