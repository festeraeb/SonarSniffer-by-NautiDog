#!/usr/bin/env python3
"""Measure georeference drift between tile GeoTIFFs and a reference full image.

Usage examples:
  python drift_measure.py --mode roundtrip --tile tile.tif --reference full.tif
  python drift_measure.py --mode tiles_vs_ref --tiles-dir tiles/ --reference full.tif

The script samples a small NxN grid per tile (configurable) and reports mean/max/RMS drift in meters.
"""
import argparse
import json
import math
from pathlib import Path

import numpy as np
import rasterio
from rasterio.transform import Affine
from pyproj import Geod


def sample_points(height, width, n):
    rows = np.linspace(0.5, height - 0.5, n)
    cols = np.linspace(0.5, width - 0.5, n)
    pts = [(int(r), int(c)) for r in rows for c in cols]
    return pts


def world_from_pixel(transform: Affine, row: float, col: float):
    # Affine multiplication expects (col, row)
    x, y = transform * (col, row)
    return float(x), float(y)


def pixel_from_world(inv_transform: Affine, x: float, y: float):
    col, row = inv_transform * (x, y)
    return float(row), float(col)


def distance_meters(x1, y1, x2, y2, crs_is_geographic):
    if crs_is_geographic:
        # (lon, lat)
        geod = Geod(ellps="WGS84")
        az1, az2, dist = geod.inv(x1, y1, x2, y2)
        return abs(dist)
    else:
        dx = x2 - x1
        dy = y2 - y1
        return math.hypot(dx, dy)


def measure_tile_vs_ref(tile_path: Path, ref_ds, cfg):
    with rasterio.open(tile_path) as tds:
        t_transform = tds.transform
        t_h, t_w = tds.height, tds.width
        inv_ref = ~ref_ds.transform
        pts = sample_points(t_h, t_w, cfg["drift_sample_points"]) if cfg["drift_sample_points"] > 0 else [(t_h // 2, t_w // 2)]
        distances = []
        for r, c in pts:
            x_tile, y_tile = world_from_pixel(t_transform, r, c)
            # map world -> ref pixel (float)
            ref_colf, ref_rowf = (~ref_ds.transform) * (x_tile, y_tile)
            # if outside bounds skip
            if not (0 <= ref_rowf < ref_ds.height and 0 <= ref_colf < ref_ds.width):
                continue
            # compute world coords from ref pixel (subpixel allowed)
            ref_x, ref_y = ref_ds.transform * (ref_colf, ref_rowf)
            d = distance_meters(x_tile, y_tile, ref_x, ref_y, ref_ds.crs.is_geographic)
            distances.append(d)
        return distances


def summarize(distances):
    if not distances:
        return None
    arr = np.array(distances)
    return {
        "count": int(arr.size),
        "mean_m": float(arr.mean()),
        "median_m": float(np.median(arr)),
        "max_m": float(arr.max()),
        "rms_m": float(math.sqrt((arr ** 2).mean())),
    }


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--mode", choices=["roundtrip", "tiles_vs_ref"], required=True)
    p.add_argument("--tile", help="Single tile GeoTIFF path for roundtrip mode")
    p.add_argument("--tiles-dir", help="Directory containing tiles to check")
    p.add_argument("--reference", required=True, help="Reference GeoTIFF path (full image)")
    p.add_argument("--config", help="YAML config to override sample count and thresholds", default=None)
    p.add_argument("--out", help="Optional JSON report output file", default=None)
    args = p.parse_args()

    # Load config minimal defaults
    cfg = {"drift_sample_points": 9, "drift_threshold_meters": 10.0}
    if args.config:
        import yaml

        with open(args.config, "r") as fh:
            cfg.update(yaml.safe_load(fh))

    ref_ds = rasterio.open(args.reference)
    results = {}

    if args.mode == "roundtrip":
        if not args.tile:
            raise SystemExit("--tile is required in roundtrip mode")
        distances = measure_tile_vs_ref(Path(args.tile), ref_ds, cfg)
        results[str(args.tile)] = summarize(distances)
    else:
        td = Path(args.tiles_dir or ".")
        for tif in sorted(td.glob("*.tif")):
            distances = measure_tile_vs_ref(tif, ref_ds, cfg)
            results[str(tif)] = summarize(distances)

    ref_ds.close()

    # Print human readable summary
    for k, v in results.items():
        if v is None:
            print(f"{k}: no overlapping samples (skipped)")
        else:
            print(f"{k}: count={v['count']} mean={v['mean_m']:.3f}m rms={v['rms_m']:.3f}m max={v['max_m']:.3f}m")

    if args.out:
        with open(args.out, "w") as fh:
            json.dump({"results": results, "config": cfg}, fh, indent=2)


if __name__ == "__main__":
    main()
