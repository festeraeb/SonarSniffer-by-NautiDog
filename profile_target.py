#!/usr/bin/env python3
"""Interior-relief & geometry profiler for a BAG reconstruction tile.

Discriminates a vessel (interior structure, linear keel axis, hollow) from a
solid monolithic rock (flowerpot/dolomite pillar = single convex mound).

Outputs:
  - high-scale hillshade PNG of the tile
  - peak cell location (lat/lon)
  - long-axis / short-axis dims of the high-relief core + L:B ratio + orientation
  - interior-relief metrics: # local maxima, ridge linearity, central hollow test
  - two cross-section profiles (long axis, short axis)

NumPy 2.x safe (rasterio, no gdal_array).
"""
import argparse
import math
import numpy as np
import rasterio
from rasterio.warp import transform as warp_transform
from PIL import Image, ImageDraw, ImageFont


def hillshade(arr, az=315.0, alt=45.0, cell=8.0):
    az_r = math.radians(360.0 - az + 90.0)
    alt_r = math.radians(alt)
    dy, dx = np.gradient(np.nan_to_num(arr, nan=np.nanmean(arr)), cell, cell)
    slope = np.pi / 2.0 - np.arctan(np.hypot(dx, dy))
    aspect = np.arctan2(-dx, dy)
    sh = np.sin(alt_r) * np.sin(slope) + np.cos(alt_r) * np.cos(slope) * np.cos(az_r - aspect)
    return 255.0 * (sh + 1.0) / 2.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tif")
    ap.add_argument("--png", default=None)
    ap.add_argument("--scale", type=int, default=10)
    ap.add_argument("--core-pct", type=float, default=60.0,
                    help="relief percentile defining the high-relief 'core'")
    args = ap.parse_args()

    with rasterio.open(args.tif) as ds:
        z = ds.read(1).astype("float64")
        nd = ds.nodata
        if nd is not None:
            z = np.where(z == nd, np.nan, z)
        z = np.where(np.isfinite(z), z, np.nan)
        gt = ds.transform
        crs = ds.crs
        cell = abs(gt.a)
        H, W = z.shape

    finite = np.isfinite(z)
    v = z[finite]
    floor = np.nanpercentile(z, 5)          # surrounding seabed (deepest 5%)
    peak = np.nanmax(z)
    relief = peak - floor
    print("tile           : %dx%d px @ %.1f m/cell  (%.0f x %.0f m)"
          % (W, H, cell, W * cell, H * cell))
    print("depth floor(p5): %.2f m   peak: %.2f m   relief: %.2f m (%.0f ft)"
          % (floor, peak, relief, relief * 3.28084))

    # Height above local floor
    hgt = z - floor
    hgt[~finite] = np.nan

    # Peak cell -> lat/lon
    pr, pc = np.unravel_index(np.nanargmax(hgt), hgt.shape)
    x = gt.c + gt.a * (pc + 0.5) + gt.b * (pr + 0.5)
    y = gt.f + gt.d * (pc + 0.5) + gt.e * (pr + 0.5)
    lon, lat = warp_transform(crs, "EPSG:4326", [x], [y])
    plat, plon = lat[0], lon[0]
    print("peak cell      : row %d col %d -> lat %.5f lon %.5f" % (pr, pc, plat, plon))

    # High-relief core mask
    thr = np.nanpercentile(hgt, args.core_pct)
    core = (hgt >= thr) & finite
    ys, xs = np.where(core)
    if xs.size >= 5:
        # PCA on core pixel coords for long/short axis
        pts = np.column_stack([xs * cell, ys * cell]).astype("float64")
        ctr = pts.mean(0)
        cov = np.cov((pts - ctr).T)
        evals, evecs = np.linalg.eigh(cov)
        order = np.argsort(evals)[::-1]
        evals = evals[order]; evecs = evecs[:, order]
        proj = (pts - ctr) @ evecs
        long_len = proj[:, 0].max() - proj[:, 0].min()
        short_len = proj[:, 1].max() - proj[:, 1].min()
        lb = long_len / short_len if short_len > 0 else float("inf")
        ang = math.degrees(math.atan2(evecs[1, 0], evecs[0, 0]))
        print("core(@p%.0f)    : %d px  long %.0f m  beam %.0f m  L:B %.2f  axis %.0f deg"
              % (args.core_pct, xs.size, long_len, short_len, lb, ang % 180))
        print("                 (%.0f x %.0f ft)" % (long_len * 3.28084, short_len * 3.28084))
    else:
        long_len = short_len = lb = ang = 0
        print("core           : too small to fit axis")

    # ── INTERIOR-RELIEF metrics (the rock-vs-wreck discriminator) ──
    # 1) Count distinct local maxima inside the core (3x3 nbhd max).
    from numpy.lib.stride_tricks import sliding_window_view
    hp = np.nan_to_num(hgt, nan=-1e9)
    nmax = 0
    if H >= 3 and W >= 3:
        win = sliding_window_view(hp, (3, 3))
        loc = win.max(axis=(-1, -2))
        center = hp[1:-1, 1:-1]
        peaks = (center == loc) & (center > floor - floor + 0.3 * relief + 0)  # >30% relief
        # require it to be a real bump, not flat plateau
        peaks &= core[1:-1, 1:-1]
        nmax = int(peaks.sum())
    print("local maxima   : %d  (1 ~ monolith/flowerpot; >=2 ~ structure)" % nmax)

    # 2) Central hollow test: is the centroid lower than the surrounding rim?
    if xs.size >= 5:
        cy, cx = int(ys.mean()), int(xs.mean())
        r = max(2, int(min(long_len, short_len) / cell / 4))
        cc = hgt[max(0, cy - r):cy + r + 1, max(0, cx - r):cx + r + 1]
        center_h = np.nanmean(cc)
        rim = hgt[core].mean()
        hollow = center_h < rim - 0.1 * relief
        print("centroid h     : %.2f m   core-mean h: %.2f m   hollow(deck/hold)? %s"
              % (center_h, rim, "YES" if hollow else "no"))

    # 3) Relief roughness within core: std of height / mean height.
    if core.sum() > 0:
        ch = hgt[core]
        rough = np.nanstd(ch) / (np.nanmean(ch) + 1e-9)
        print("core roughness : %.3f  (low ~ smooth dome; high ~ broken/structured)" % rough)

    # 4) Long-axis & short-axis cross-section profiles through the peak.
    def profile(line_row):
        row = hgt[line_row, :]
        return row
    print("\nlong-axis profile (height above floor, m), row=%d:" % pr)
    prof = hgt[pr, :]
    pf = prof[np.isfinite(prof)]
    if pf.size:
        # compact sparkline
        spark = "▁▂▃▄▅▆▇█"
        mn, mx = 0.0, max(1e-6, np.nanmax(prof))
        line = "".join(spark[min(7, max(0, int((x - mn) / (mx - mn) * 7)))]
                        if np.isfinite(x) else " " for x in prof)
        print("  " + line)
    print("short-axis profile, col=%d:" % pc)
    profc = hgt[:, pc]
    if np.isfinite(profc).any():
        mx = max(1e-6, np.nanmax(profc))
        line = "".join(spark[min(7, max(0, int((x) / mx * 7)))]
                        if np.isfinite(x) else " " for x in profc)
        print("  " + line)

    # ── render hillshade PNG with core outline + peak pin ──
    out = args.png or (args.tif.rsplit(".", 1)[0] + "_profile.png")
    sh = hillshade(z, cell=cell)
    g = np.clip((sh - np.nanpercentile(sh, 2)) /
                (np.nanpercentile(sh, 98) - np.nanpercentile(sh, 2) + 1e-9), 0, 1) * 255
    rgb = np.zeros((H, W, 3), np.uint8)
    for c in range(3):
        rgb[..., c] = np.where(finite, g.astype(np.uint8), [10, 18, 40][c])
    # tint the core faint red
    rgb[core, 0] = np.clip(rgb[core, 0].astype(int) + 60, 0, 255)
    img = Image.fromarray(rgb, "RGB")
    s = args.scale
    img = img.resize((W * s, H * s), Image.NEAREST)
    d = ImageDraw.Draw(img)
    d.line([(pc * s - 14, pr * s), (pc * s + 14, pr * s)], fill=(255, 220, 0), width=2)
    d.line([(pc * s, pr * s - 14), (pc * s, pr * s + 14)], fill=(255, 220, 0), width=2)
    img.save(out)
    print("\nwrote", out, img.size)


if __name__ == "__main__":
    main()
