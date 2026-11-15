"""Render a GLB from a few orthographic views (shaded normals or base-colour texture) into one PNG.

    python.exe -s render_preview.py model.glb preview.png [--size 384]
Uses the custom_rasterizer shim (nvdiffrast) from ../shims.
"""

import argparse
import os
import sys

import numpy as np
import torch

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "shims"))
import custom_rasterizer as cr  # noqa: E402


def look_at_views(n_az=4, elev_deg=15.0):
    views = []
    for k in range(n_az):
        az = np.radians(k * 360.0 / n_az)
        el = np.radians(elev_deg)
        eye = np.array([np.sin(az) * np.cos(el), np.sin(el), np.cos(az) * np.cos(el)]) * 3.0
        f = -eye / np.linalg.norm(eye)
        r = np.cross(f, [0, 1, 0]); r /= np.linalg.norm(r)
        u = np.cross(r, f)
        m = np.eye(4)
        m[0, :3], m[1, :3], m[2, :3] = r, u, -f
        m[:3, 3] = -m[:3, :3] @ eye
        views.append(m)
    return views


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("glb")
    ap.add_argument("png")
    ap.add_argument("--size", type=int, default=384)
    a = ap.parse_args()
    import trimesh
    from PIL import Image
    scene = trimesh.load(a.glb)
    mesh = scene.dump(concatenate=True) if isinstance(scene, trimesh.Scene) else scene
    v = np.asarray(mesh.vertices, dtype=np.float64)
    center = (v.max(0) + v.min(0)) / 2
    scale = 1.0 / np.linalg.norm(v - center, axis=1).max()
    v = (v - center) * scale
    dev = "cuda"
    tri = torch.as_tensor(np.asarray(mesh.faces), dtype=torch.int32, device=dev)
    normals = torch.as_tensor(np.asarray(mesh.vertex_normals), dtype=torch.float32, device=dev)
    uv = tex = None
    vis = getattr(mesh, "visual", None)
    if vis is not None and getattr(vis, "uv", None) is not None and getattr(vis, "material", None) is not None:
        img = getattr(vis.material, "baseColorTexture", None) or getattr(vis.material, "image", None)
        if img is not None:
            uv = torch.as_tensor(np.asarray(vis.uv), dtype=torch.float32, device=dev)
            tex = torch.as_tensor(np.asarray(img.convert("RGB")), dtype=torch.float32, device=dev) / 255.0
    tiles = []
    S = a.size
    for m in look_at_views():
        vh = np.c_[v, np.ones(len(v))] @ m.T
        clip = vh.copy()
        clip[:, 0] /= 1.1; clip[:, 1] /= 1.1
        clip[:, 2] = -(vh[:, 2] + 3.0) / 3.0  # depth in [-1, 1] roughly, nearer = smaller
        clip[:, 3] = 1.0
        pos = torch.as_tensor(clip, dtype=torch.float32, device=dev)[None]
        f, b = cr.rasterize(pos, tri, (S, S))
        mask = (f > 0)[..., None].float()
        n_cam = normals @ torch.as_tensor(m[:3, :3].T, dtype=torch.float32, device=dev)
        nn = cr.interpolate(n_cam[None], f, b, tri)[0]
        nn = torch.nn.functional.normalize(nn, dim=-1)
        shade = (0.25 + 0.75 * nn[..., 2:3].clamp(min=0))
        if tex is not None:
            t = cr.interpolate(uv[None], f, b, tri)[0]
            H, W = tex.shape[:2]
            x = (t[..., 0].clamp(0, 1) * (W - 1)).round().long()
            y = ((1 - t[..., 1]).clamp(0, 1) * (H - 1)).round().long()
            col = tex[y, x] * (0.55 + 0.45 * shade)
        else:
            col = shade.expand(-1, -1, 3) * torch.tensor([0.85, 0.85, 0.9], device=dev)
        img = col * mask + (1 - mask) * 1.0
        img = torch.flip(img, dims=[0])  # row 0 = NDC y=-1 -> flip for an upright PNG
        tiles.append((img.clamp(0, 1).cpu().numpy() * 255).astype(np.uint8))
    Image.fromarray(np.concatenate(tiles, axis=1)).save(a.png)
    print(a.png, len(mesh.faces), "faces")


if __name__ == "__main__":
    main()
