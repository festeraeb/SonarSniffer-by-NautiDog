#!/usr/bin/env python3
"""Detrend a reconstruction tile and isolate the raised OBJECT from the seabed.

The 4-8 m BAG tiles around a target include a sloping/curved seabed; a simple
percentile threshold over the whole tile measures the tile, not the object.
This subtracts a large-window smooth background (the regional seabed), then
flood-fills a connected region outward from the peak to isolate the structure,
and measures it.

Rock-vs-wreck discriminators reported:
  - L:B ratio of the isolated object (vessel ~3-5:1; flowerpot/boulder ~1:1)
  - # interior local maxima within the object (monolith=1; hull=multiple)
  - longitudinal ridge profile (keel line) vs single convex dome
NumPy 2.x safe.
"""
import argparse, math
import numpy as np
import rasterio
from rasterio.warp import transform as warp_transform
from PIL import Image, ImageDraw


def smooth_box(a, k):
    """Mean filter ignoring NaN, window (2k+1)."""
    m = np.isfinite(a)
    af = np.where(m, a, 0.0)
    # integral-image box sum
    from numpy.lib.stride_tricks import sliding_window_view
    pad = k
    ap = np.pad(af, pad, mode="reflect")
    mp = np.pad(m.astype("float64"), pad, mode="reflect")
    win_a = sliding_window_view(ap, (2 * k + 1, 2 * k + 1))
    win_m = sliding_window_view(mp, (2 * k + 1, 2 * k + 1))
    s = win_a.sum((-1, -2))
    c = win_m.sum((-1, -2))
    out = np.where(c > 0, s / np.maximum(c, 1), np.nan)
    return out


def grow(resid, seed, lo):
    """Flood fill from seed over cells with resid >= lo (8-connected)."""
    H, W = resid.shape
    seen = np.zeros((H, W), bool)
    stack = [seed]
    seen[seed] = True
    comp = []
    while stack:
        r, c = stack.pop()
        comp.append((r, c))
        for dr in (-1, 0, 1):
            for dc in (-1, 0, 1):
                nr, nc = r + dr, c + dc
                if 0 <= nr < H and 0 <= nc < W and not seen[nr, nc]:
                    if np.isfinite(resid[nr, nc]) and resid[nr, nc] >= lo:
                        seen[nr, nc] = True
                        stack.append((nr, nc))
    return comp


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tif")
    ap.add_argument("--png", default=None)
    ap.add_argument("--bg-window-m", type=float, default=200.0,
                    help="background smoothing window (m); >> object size")
    ap.add_argument("--object-frac", type=float, default=0.30,
                    help="grow object down to this fraction of its peak residual")
    ap.add_argument("--scale", type=int, default=14)
    args = ap.parse_args()

    with rasterio.open(args.tif) as ds:
        z = ds.read(1).astype("float64")
        nd = ds.nodata
        if nd is not None:
            z = np.where(z == nd, np.nan, z)
        z = np.where(np.isfinite(z), z, np.nan)
        gt = ds.transform; crs = ds.crs
        cell = abs(gt.a); H, W = z.shape

    k = max(3, int(args.bg_window_m / cell / 2))
    bg = smooth_box(z, k)
    resid = z - bg                       # + = raised above regional seabed
    resid[~np.isfinite(z)] = np.nan

    rmax = np.nanmax(resid)
    pr, pc = np.unravel_index(np.nanargmax(np.nan_to_num(resid, nan=-1e9)), resid.shape)
    lo = args.object_frac * rmax
    comp = grow(resid, (pr, pc), lo)
    ys = np.array([c[0] for c in comp]); xs = np.array([c[1] for c in comp])

    # peak -> lat/lon
    x = gt.c + gt.a * (pc + 0.5); y = gt.f + gt.e * (pr + 0.5)
    lon, lat = warp_transform(crs, "EPSG:4326", [x], [y])
    print("tile           : %dx%d @ %.1f m (bg window %d m)" % (W, H, cell, (2*k+1)*int(cell)))
    print("object residual: peak +%.2f m above seabed (%.0f ft)  grow>=+%.2f m"
          % (rmax, rmax * 3.28084, lo))
    print("peak cell      : lat %.5f lon %.5f" % (lat[0], lon[0]))
    print("object pixels  : %d  (%.0f m^2)" % (len(comp), len(comp) * cell * cell))

    if xs.size >= 5:
        pts = np.column_stack([xs * cell, ys * cell]).astype("float64")
        ctr = pts.mean(0); cov = np.cov((pts - ctr).T)
        evals, evecs = np.linalg.eigh(cov); o = np.argsort(evals)[::-1]
        evecs = evecs[:, o]; proj = (pts - ctr) @ evecs
        L = proj[:, 0].max() - proj[:, 0].min()
        B = proj[:, 1].max() - proj[:, 1].min()
        lb = L / B if B > 0 else float("inf")
        ang = math.degrees(math.atan2(evecs[1, 0], evecs[0, 0])) % 180
        print("object dims    : long %.0f m (%.0f ft)  beam %.0f m (%.0f ft)  L:B %.2f  axis %.0f deg"
              % (L, L*3.28084, B, B*3.28084, lb, ang))
        verdict = "VESSEL-like (elongated)" if lb >= 2.5 else \
                  ("possible wreck" if lb >= 1.8 else "BOULDER/monolith-like (~round)")
        print("shape verdict  : L:B %.2f -> %s" % (lb, verdict))

        # interior maxima within object
        objmask = np.zeros((H, W), bool); objmask[ys, xs] = True
        from numpy.lib.stride_tricks import sliding_window_view
        rp = np.nan_to_num(resid, nan=-1e9)
        win = sliding_window_view(rp, (3, 3)).max((-1, -2))
        cen = rp[1:-1, 1:-1]
        loc = (cen == win) & objmask[1:-1, 1:-1] & (cen > lo)
        print("interior maxima: %d (1=monolith/flowerpot; >=2=internal structure)"
              % int(loc.sum()))

        # keel-line longitudinal profile (sample resid along long axis)
        n = 40
        ts = np.linspace(proj[:, 0].min(), proj[:, 0].max(), n)
        spark = "▁▂▃▄▅▆▇█"
        prof = []
        for t in ts:
            wp = ctr + t * evecs[:, 0]          # metres
            cc = int(wp[0] / cell); rr = int(wp[1] / cell)
            prof.append(resid[rr, cc] if (0 <= rr < H and 0 <= cc < W) else np.nan)
        prof = np.array(prof); mx = max(1e-6, np.nanmax(prof))
        line = "".join(spark[min(7, max(0, int(p/mx*7)))] if np.isfinite(p) and p > 0 else " "
                       for p in prof)
        print("keel profile   : %s" % line)
        print("                 (flat-top/multi-bump = deck/structure; single peak = dome)")

    # render
    out = args.png or (args.tif.rsplit(".", 1)[0] + "_object.png")
    rr = resid.copy(); finite = np.isfinite(rr)
    g = np.clip((rr - 0) / (max(1e-6, rmax)), 0, 1) * 255
    rgb = np.zeros((H, W, 3), np.uint8)
    rgb[..., 0] = np.where(finite, g.astype(np.uint8), 10)
    rgb[..., 1] = np.where(finite, g.astype(np.uint8), 18)
    rgb[..., 2] = np.where(finite, (g*0.6).astype(np.uint8), 40)
    if xs.size:
        rgb[ys, xs, 1] = np.clip(rgb[ys, xs, 1].astype(int) + 90, 0, 255)  # green object
    img = Image.fromarray(rgb, "RGB").resize((W*args.scale, H*args.scale), Image.NEAREST)
    d = ImageDraw.Draw(img); s = args.scale
    d.line([(pc*s-16, pr*s), (pc*s+16, pr*s)], fill=(255, 220, 0), width=2)
    d.line([(pc*s, pr*s-16), (pc*s, pr*s+16)], fill=(255, 220, 0), width=2)
    img.save(out)
    print("wrote", out, img.size)


if __name__ == "__main__":
    main()
