#!/usr/bin/env python3
"""Render a BAG reconstruction GeoTIFF to PNG and pin lat/lon markers on it.

Utility for visually placing wreck candidates / features in relation to the
larger reconstructed surface (e.g. an old riverbed).

Usage:
    python pin_on_mask.py <recon.tif> <out.png> [--hillshade] \
        --pin LAT,LON,LABEL [--pin LAT,LON,LABEL ...]

Renders grayscale depth (or hillshade) and draws a labeled cross-hair at each
pinned lat/lon. Works under NumPy 2.x (uses rasterio, not gdal_array).
"""
import argparse
import math
import numpy as np
import rasterio
from rasterio.warp import transform as warp_transform
from PIL import Image, ImageDraw, ImageFont


def hillshade(arr, az=315.0, alt=45.0, cell=8.0):
    """Horn hillshade (0..255). NaNs stay NaN."""
    az_r = math.radians(360.0 - az + 90.0)
    alt_r = math.radians(alt)
    dy, dx = np.gradient(arr, cell, cell)
    slope = np.pi / 2.0 - np.arctan(np.hypot(dx, dy))
    aspect = np.arctan2(-dx, dy)
    shaded = (np.sin(alt_r) * np.sin(slope)
              + np.cos(alt_r) * np.cos(slope) * np.cos(az_r - aspect))
    return (255.0 * (shaded + 1.0) / 2.0)


def to_gray(arr):
    """Stretch finite values to 0..255 (2-98 pct)."""
    v = arr[np.isfinite(arr)]
    if v.size == 0:
        return np.zeros_like(arr, dtype=np.uint8), np.zeros(arr.shape, bool)
    lo, hi = np.percentile(v, 2), np.percentile(v, 98)
    if hi <= lo:
        hi = lo + 1.0
    g = np.clip((arr - lo) / (hi - lo), 0, 1) * 255.0
    return g, np.isfinite(arr)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tif")
    ap.add_argument("out")
    ap.add_argument("--hillshade", action="store_true")
    ap.add_argument("--scale", type=int, default=3, help="upscale factor for legibility")
    ap.add_argument("--pin", action="append", default=[],
                    help="LAT,LON,LABEL (repeatable)")
    ap.add_argument("--box", action="append", default=[],
                    help="LAT1,LON1,LAT2,LON2,LABEL bounding box (repeatable)")
    args = ap.parse_args()

    with rasterio.open(args.tif) as ds:
        band = ds.read(1).astype("float64")
        nodata = ds.nodata
        if nodata is not None:
            band = np.where(band == nodata, np.nan, band)
        band = np.where(np.isfinite(band), band, np.nan)
        gt = ds.transform
        crs = ds.crs
        H, W = band.shape
        cell = abs(gt.a)

        if args.hillshade:
            shade = hillshade(band, cell=cell)
            g, mask = to_gray(shade)
        else:
            g, mask = to_gray(band)

    # Base RGB; NaN -> dark blue (water/void)
    rgb = np.zeros((H, W, 3), dtype=np.uint8)
    gi = g.astype(np.uint8)
    for c in range(3):
        rgb[..., c] = np.where(mask, gi, [10, 18, 40][c])
    img = Image.fromarray(rgb, "RGB")

    s = max(1, args.scale)
    if s > 1:
        img = img.resize((W * s, H * s), Image.NEAREST)
    draw = ImageDraw.Draw(img)
    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 14)
    except Exception:
        font = ImageFont.load_default()

    # invert geotransform to map world->pixel
    inv = ~gt
    colors = [(255, 60, 60), (60, 200, 255), (255, 220, 0), (120, 255, 120)]

    # Draw feature footprint boxes first (so pins sit on top).
    box_colors = [(0, 255, 180), (255, 140, 0), (200, 120, 255)]
    for i, spec in enumerate(args.box):
        parts = spec.split(",")
        lat1, lon1, lat2, lon2 = map(float, parts[:4])
        label = parts[4] if len(parts) > 4 else ""
        xs, ys = warp_transform("EPSG:4326", crs,
                                [lon1, lon2, lon1, lon2],
                                [lat1, lat2, lat2, lat1])
        cols, rows = [], []
        for x, y in zip(xs, ys):
            c, r = inv * (x, y)
            cols.append(c * s)
            rows.append(r * s)
        x0, x1 = min(cols), max(cols)
        y0, y1 = min(rows), max(rows)
        bc = box_colors[i % len(box_colors)]
        draw.rectangle([x0, y0, x1, y1], outline=bc, width=3)
        if label:
            draw.text((x0 + 4, y0 + 4), label, fill=bc, font=font)
        print("boxed  %-18s [%.4f,%.4f]-[%.4f,%.4f]" % (label, lat1, lon1, lat2, lon2))

    for i, spec in enumerate(args.pin):
        parts = spec.split(",")
        lat, lon = float(parts[0]), float(parts[1])
        label = parts[2] if len(parts) > 2 else ""
        # WGS84 -> raster CRS
        xs, ys = warp_transform("EPSG:4326", crs, [lon], [lat])
        col, row = inv * (xs[0], ys[0])
        px, py = col * s, row * s
        col_rgb = colors[i % len(colors)]
        r = 10
        draw.line([(px - r, py), (px + r, py)], fill=col_rgb, width=2)
        draw.line([(px, py - r), (px, py + r)], fill=col_rgb, width=2)
        draw.ellipse([px - r, py - r, px + r, py + r], outline=col_rgb, width=2)
        if label:
            draw.text((px + r + 3, py - 8), label, fill=col_rgb, font=font)
        print("pinned %-18s lat=%.5f lon=%.5f -> px=(%.0f,%.0f)%s"
              % (label, lat, lon, col, row,
                 "" if (0 <= col < W and 0 <= row < H) else "  [OUT OF FRAME]"))

    img.save(args.out)
    print("wrote", args.out, img.size)


if __name__ == "__main__":
    main()
