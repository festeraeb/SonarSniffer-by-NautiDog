#!/usr/bin/env python3
"""Fetch Sentinel-2 L2A band COGs from a Rust-pipeline STAC manifest.

The Rust download stage writes a ranked, calm-gated manifest
(sentinel2_stac_manifest.json) but delegates the actual band fetch. This pulls
the band COGs from the free Element84 AWS public bucket (no auth), in manifest
rank order (perfect-day scenes first), up to a file budget.

Band naming matches the existing local tiles: <scene_id>.<band>.tif
Default bands B02/B03/B04/B11 = blue/green/red/swir16 (penetrating + plume).

Usage:
  python fetch_s2_from_manifest.py <manifest.json> <out_dir> --max-files 40
  python fetch_s2_from_manifest.py ... --bands blue,green,red,swir16
"""
import argparse, json, os, sys, time
import urllib.request

# Element84 earth-search v1 asset keys -> our local band suffix
BAND_ASSET = {
    "blue": "blue", "green": "green", "red": "red", "nir": "nir",
    "nir08": "nir08", "swir16": "swir16", "swir22": "swir22", "scl": "scl",
}

def http_size(url):
    try:
        req = urllib.request.Request(url, method="HEAD")
        with urllib.request.urlopen(req, timeout=20) as r:
            return int(r.headers.get("Content-Length", 0)), r.status
    except Exception as e:
        return 0, str(e)

def download(url, dest):
    tmp = dest + ".part"
    with urllib.request.urlopen(url, timeout=120) as r, open(tmp, "wb") as f:
        while True:
            chunk = r.read(1 << 20)
            if not chunk:
                break
            f.write(chunk)
    os.replace(tmp, dest)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("manifest")
    ap.add_argument("out_dir")
    ap.add_argument("--bands", default="blue,green,red,swir16")
    ap.add_argument("--max-files", type=int, default=40)
    ap.add_argument("--perfect-first", action="store_true", default=True)
    args = ap.parse_args()

    scenes = json.load(open(args.manifest))
    bands = [b.strip() for b in args.bands.split(",") if b.strip()]
    os.makedirs(args.out_dir, exist_ok=True)

    # Manifest is already rank-sorted (perfect-day first); keep that order.
    budget = args.max_files
    fetched = skipped = failed = 0
    log = []
    for s in scenes:
        if budget <= 0:
            break
        sid = s["id"]
        assets = s.get("assets", {})
        for band in bands:
            if budget <= 0:
                break
            # asset href: prefer manifest-provided, else derive from earth-search
            href = assets.get(band) or assets.get(BAND_ASSET.get(band, band))
            if not href:
                log.append(f"[no-asset] {sid} {band}")
                continue
            dest = os.path.join(args.out_dir, f"{sid}.{band}.tif")
            if os.path.exists(dest) and os.path.getsize(dest) > 1_000_000:
                skipped += 1
                budget -= 1
                continue
            sz, status = http_size(href)
            if status != 200:
                log.append(f"[head {status}] {sid} {band} {href}")
                failed += 1
                continue
            try:
                t0 = time.time()
                download(href, dest)
                dt = time.time() - t0
                mb = os.path.getsize(dest) / 1e6
                print(f"[ok] {sid}.{band}.tif {mb:.1f}MB {dt:.1f}s "
                      f"(cloud {s.get('cloud_cover')}, perfect={s.get('perfect_day')})")
                fetched += 1
                budget -= 1
            except Exception as e:
                log.append(f"[err] {sid} {band}: {e}")
                failed += 1

    print(f"\nfetched={fetched} skipped={skipped} failed={failed} "
          f"budget_left={budget}")
    for l in log[:40]:
        print(" ", l)

if __name__ == "__main__":
    main()
