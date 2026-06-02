#!/usr/bin/env python3
from pathlib import Path
import argparse
import rasterio


def intersects(b1, b2):
    # b: left, bottom, right, top
    return not (b1[2] < b2[0] or b1[0] > b2[2] or b1[3] < b2[1] or b1[1] > b2[3])


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--reference", required=True)
    p.add_argument("--tiles", required=True)
    args = p.parse_args()

    ref = rasterio.open(args.reference)
    ref_bounds = (ref.bounds.left, ref.bounds.bottom, ref.bounds.right, ref.bounds.top)
    ref_crs = ref.crs
    print(f"Reference: {args.reference}")
    print(f"  bounds: {ref_bounds}")
    print(f"  crs: {ref_crs}")

    tiles_dir = Path(args.tiles)
    overlapping = []
    total = 0
    for tif in sorted(tiles_dir.rglob("*.tif")):
        total += 1
        try:
            ds = rasterio.open(tif)
            b = (ds.bounds.left, ds.bounds.bottom, ds.bounds.right, ds.bounds.top)
            if ds.crs != ref_crs:
                # attempt to compare if crs differs — treat as non-overlap
                overlaps = False
            else:
                overlaps = intersects(b, ref_bounds)
            if overlaps:
                overlapping.append(str(tif))
        except Exception:
            continue

    print(f"Scanned {total} tiles; overlapping: {len(overlapping)}")
    for o in overlapping:
        print(" -", o)


if __name__ == '__main__':
    main()
