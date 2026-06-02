#!/usr/bin/env python3
"""
collect_and_process.py

Proof-of-concept orchestration to find GeoTIFFs on this machine, run GPU audits,
cluster and group detections, and write consolidated outputs to a single folder.

This script reuses existing pipeline modules in the repo and requires Python deps
already used by the project (rasterio, pandas, numpy). It will attempt to use
the GPU path via `gpu_batch_runner` and fall back to CPU where available.
"""
import argparse
import os
from pathlib import Path
import json
import datetime

def find_tiffs(input_dir):
    p = Path(input_dir)
    exts = ('.tif', '.tiff', '.TIF', '.TIFF')
    tiffs = [str(f) for f in p.rglob('*') if f.suffix in exts]
    return sorted(tiffs)

def ensure_outdir(base):
    p = Path(base)
    p.mkdir(parents=True, exist_ok=True)
    return p

def run_pipeline_on_tile(tiff, outdir):
    # Run existing GPU audit if available
    try:
        from wreckhunter2000.scripts.tools.gpu_batch_runner import run_multi_profile_audit
    except Exception as e:
        print(f"[WARN] gpu_batch_runner not available: {e}")
        run_multi_profile_audit = None

    try:
        if run_multi_profile_audit:
            hits = run_multi_profile_audit(tiff, cooldown_sec=1)
        else:
            # Minimal fallback: use signature_audit to test a center point
            from wreckhunter2000.tools.signature_audit import signature_audit
            # get center lat/lon via coord_sync
            from wreckhunter2000.scripts.tools.coord_sync import UniversalCoordSync
            sync = UniversalCoordSync(tiff)
            # choose center pixel
            center_row =  sync_path = None
            # best-effort: open with rasterio
            import rasterio
            with rasterio.open(tiff) as src:
                center_row = src.height // 2
                center_col = src.width // 2
                lon, lat = src.transform * (center_col, center_row)
            res = signature_audit(tiff, lat, lon)
            hits = [res]
    except Exception as e:
        print(f"[ERROR] processing {tiff}: {e}")
        hits = []

    # Save per-tile manifest
    tile_name = Path(tiff).stem
    out_json = Path(outdir) / f"{tile_name}_hits.json"
    with open(out_json, 'w') as f:
        json.dump({'tile': tiff, 'hits': hits}, f, indent=2)

    return hits

def aggregate_results(outdir, aggregated_name):
    # collect all *_hits.json files
    p = Path(outdir)
    files = list(p.glob('*_hits.json'))
    all_hits = []
    for f in files:
        try:
            j = json.load(open(f))
            all_hits.extend(j.get('hits', []))
        except Exception:
            continue

    agg_path = p / aggregated_name
    with open(agg_path, 'w') as f:
        json.dump({'run_at': datetime.datetime.utcnow().isoformat(), 'hits': all_hits}, f, indent=2)
    return agg_path

def postprocess(aggregated_json, outdir):
    # cluster and group using existing tools
    try:
        from wreckhunter2000.tools.cluster_detections import cluster_detections
        from wreckhunter2000.tools.group_thermal_detections import group_detections
    except Exception as e:
        print(f"[WARN] postprocessing modules missing: {e}")
        return

    clustered = Path(outdir) / 'clustered_detections.json'
    grouped = Path(outdir) / 'grouped_detections.json'
    cluster_detections(aggregated_json, clustered, distance_threshold=1000, zscore_tolerance=1.5)
    group_detections(clustered, grouped)
    print(f"Postprocess outputs: {clustered}, {grouped}")

def main():
    parser = argparse.ArgumentParser(description='Collect & process satellite TIFFs into a single place')
    parser.add_argument('--input-dir', '-i', type=str, required=True, help='Directory to scan for TIFFs')
    parser.add_argument('--out-dir', '-o', type=str, default='outputs/satellite_processed', help='Output base dir')
    parser.add_argument('--limit', type=int, default=0, help='Limit number of tiles to process (0 = all)')
    args = parser.parse_args()

    tiffs = find_tiffs(args.input_dir)
    if not tiffs:
        print(f"No TIFFs found under {args.input_dir}")
        return

    out_base = ensure_outdir(args.out_dir)
    run_id = datetime.datetime.utcnow().strftime('%Y%m%dT%H%M%SZ')
    run_dir = out_base / run_id
    run_dir.mkdir(parents=True, exist_ok=True)

    limit = args.limit or len(tiffs)
    tiffs = tiffs[:limit]

    print(f"Found {len(tiffs)} TIFFs; processing up to {limit} tiles")

    for i, t in enumerate(tiffs, 1):
        print(f"[{i}/{len(tiffs)}] Processing {t}")
        try:
            run_pipeline_on_tile(t, run_dir)
        except Exception as e:
            print(f"Error on {t}: {e}")

    agg = aggregate_results(run_dir, 'aggregated_hits.json')
    postprocess(agg, run_dir)
    print(f"Run complete. Outputs under: {run_dir}")

if __name__ == '__main__':
    main()
