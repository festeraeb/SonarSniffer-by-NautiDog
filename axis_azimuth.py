#!/usr/bin/env python3
"""Report each isolated feature's long-axis TRUE azimuth and compare to the
channel/ice-flow bearing. Ice-carved features (drumlins, flutes, riegels) align
with flow; a wreck settles at an arbitrary heading.
NumPy 2.x safe.
"""
import sys, math
import numpy as np
import rasterio
from rasterio.warp import transform as warp_transform


def smooth_box(a, k):
    from numpy.lib.stride_tricks import sliding_window_view
    m = np.isfinite(a); af = np.where(m, a, 0.0)
    ap = np.pad(af, k, mode="reflect"); mp = np.pad(m.astype(float), k, mode="reflect")
    s = sliding_window_view(ap, (2*k+1, 2*k+1)).sum((-1, -2))
    c = sliding_window_view(mp, (2*k+1, 2*k+1)).sum((-1, -2))
    return np.where(c > 0, s/np.maximum(c, 1), np.nan)


def grow(resid, seed, lo):
    H, W = resid.shape; seen = np.zeros((H, W), bool); st = [seed]; seen[seed] = True; comp = []
    while st:
        r, c = st.pop(); comp.append((r, c))
        for dr in (-1, 0, 1):
            for dc in (-1, 0, 1):
                nr, nc = r+dr, c+dc
                if 0 <= nr < H and 0 <= nc < W and not seen[nr, nc] \
                   and np.isfinite(resid[nr, nc]) and resid[nr, nc] >= lo:
                    seen[nr, nc] = True; st.append((nr, nc))
    return comp


def feature_azimuth(tif, bg_m=160.0, frac=0.40):
    with rasterio.open(tif) as ds:
        z = ds.read(1).astype("float64"); nd = ds.nodata
        if nd is not None: z = np.where(z == nd, np.nan, z)
        z = np.where(np.isfinite(z), z, np.nan)
        gt = ds.transform; crs = ds.crs; cell = abs(gt.a)
    k = max(3, int(bg_m/cell/2)); resid = z - smooth_box(z, k); resid[~np.isfinite(z)] = np.nan
    rmax = np.nanmax(resid)
    pr, pc = np.unravel_index(np.nanargmax(np.nan_to_num(resid, nan=-1e9)), resid.shape)
    comp = grow(resid, (pr, pc), frac*rmax)
    ys = np.array([c[0] for c in comp]); xs = np.array([c[1] for c in comp])
    if xs.size < 5:
        return None
    # Build metric vectors: East = +col, North = -row (row increases southward)
    E = xs*cell; N = -ys*cell
    pts = np.column_stack([E, N]).astype("float64"); ctr = pts.mean(0)
    cov = np.cov((pts-ctr).T); ev, evec = np.linalg.eigh(cov); o = np.argsort(ev)[::-1]
    ev = ev[o]; evec = evec[:, o]; proj = (pts-ctr) @ evec
    L = proj[:, 0].max()-proj[:, 0].min(); B = proj[:, 1].max()-proj[:, 1].min()
    lb = L/B if B > 0 else float("inf")
    # azimuth of long axis: vector (E,N) -> compass deg (0=N, 90=E)
    eE, eN = evec[0, 0], evec[1, 0]
    az = math.degrees(math.atan2(eE, eN)) % 180.0
    return dict(L=L, B=B, lb=lb, az=az, n=xs.size, relief=rmax)


def bearing(la1, lo1, la2, lo2):
    p1, p2 = math.radians(la1), math.radians(la2); dl = math.radians(lo2-lo1)
    x = math.sin(dl)*math.cos(p2)
    y = math.cos(p1)*math.sin(p2)-math.sin(p1)*math.cos(p2)*math.cos(dl)
    return math.degrees(math.atan2(x, y)) % 180.0


D = sys.argv[1]
# channel axis: candidate -> channel core
chan = bearing(45.86890, -84.59336, 45.85188, -84.57163)
print("channel/ice-flow bearing (mod 180): %.0f deg\n" % chan)
print("%-10s %6s %6s %6s %7s %5s %s" % ("feature", "long_m", "beam_m", "L:B", "azim", "n", "delta-from-channel"))
for m, lab in [("mask013", "CANDIDATE"), ("mask194", "SE-1"), ("mask079", "SE-2")]:
    r = feature_azimuth("%s/H13255_%s_recon.tif" % (D, m))
    if r is None:
        print("%-10s  (core too small to fit)" % m); continue
    d = abs(r["az"]-chan); d = min(d, 180-d)
    print("%-10s %6.0f %6.0f %6.2f %6.0f° %5d   %3.0f° %s"
          % (m+" "+lab, r["L"], r["B"], r["lb"], r["az"], r["n"], d,
             "ALIGNED w/ channel (natural?)" if d < 25 else
             "OFF-axis (wreck-like?)" if d > 45 else "partial"))
