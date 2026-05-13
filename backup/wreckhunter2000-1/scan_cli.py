#!/usr/bin/env python3
"""
CESAROPS Scan CLI — Manual scan interface

Usage:
    python scan_cli.py --lake erie --dates 2024-06-01 2024-06-30
    python scan_cli.py --bbox 41.3 -83.5 42.5 -78.8 --dates 2024-06-01 2024-06-30
    python scan_cli.py --lake michigan --dates 2024-07-01 2024-07-31 --download --sensitivity high
    python scan_cli.py --lake straits --dates 2015-10-01 2015-10-31 --passes standard_anomaly hydrocarbon
"""

import argparse
import sys
from pathlib import Path

from scan_engine import ScanEngine, LAKE_PRESETS, DEFAULT_PASS_CONFIG


SENSITIVITY_PRESETS = {
    "low": {
        "standard_anomaly": {"thermal_thresh": 3.0, "blue_thresh": 2.0, "default_thresh": 2.5},
    },
    "normal": {},  # use defaults
    "high": {
        "standard_anomaly": {"thermal_thresh": 1.5, "blue_thresh": 0.8, "default_thresh": 1.0, "top_n": 500},
        "hydrocarbon": {"swir_thresh": -1.2, "red_thresh": 1.0},
    },
}


def main():
    p = argparse.ArgumentParser(
        description="CESAROPS Scan Engine CLI",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=f"Available lakes: {', '.join(LAKE_PRESETS.keys())}\n"
               f"Available passes: {', '.join(DEFAULT_PASS_CONFIG.keys())}",
    )

    # Area selection (mutually exclusive)
    area = p.add_mutually_exclusive_group(required=True)
    area.add_argument("--lake", choices=list(LAKE_PRESETS.keys()),
                      help="Lake preset name")
    area.add_argument("--bbox", nargs=4, type=float,
                      metavar=("LAT_MIN", "LON_MIN", "LAT_MAX", "LON_MAX"),
                      help="Bounding box [lat_min lon_min lat_max lon_max]")

    # Date range
    p.add_argument("--dates", nargs=2, required=True,
                   metavar=("START", "END"),
                   help="Date range YYYY-MM-DD YYYY-MM-DD")

    # Options
    p.add_argument("--output", "-o", type=str, default=None,
                   help="Output directory (default: outputs/<label>)")
    p.add_argument("--data-dirs", nargs="+", type=str, default=None,
                   help="Directories to search for TIFF files")
    p.add_argument("--passes", nargs="+", type=str, default=None,
                   help="Enable only these detection passes")
    p.add_argument("--sensitivity", choices=["low", "normal", "high"],
                   default="normal",
                   help="Sensitivity preset (adjusts thresholds)")
    p.add_argument("--download", action="store_true",
                   help="Download missing data before scanning")
    p.add_argument("--label", type=str, default=None,
                   help="Custom label for this scan run")
    p.add_argument("--dry-run", action="store_true",
                   help="Show what would run without executing")

    args = p.parse_args()

    # Build pass config
    pass_overrides = {}

    # Sensitivity preset
    if args.sensitivity != "normal":
        pass_overrides = dict(SENSITIVITY_PRESETS[args.sensitivity])

    # If --passes specified, disable all others
    if args.passes:
        for k in DEFAULT_PASS_CONFIG:
            if k not in args.passes:
                pass_overrides.setdefault(k, {})["enabled"] = False

    # Download if requested
    if args.download:
        _download_data(args)

    # Print config
    lake = args.lake
    bbox = args.bbox or LAKE_PRESETS[lake]["bbox"]
    label = args.label or (LAKE_PRESETS[lake]["label"] if lake else f"scan_{args.dates[0]}")

    print("=" * 60)
    print(f"CESAROPS SCAN — {label}")
    print("=" * 60)
    print(f"  Area:        {lake or bbox}")
    print(f"  BBox:        {bbox}")
    print(f"  Dates:       {args.dates[0]} → {args.dates[1]}")
    print(f"  Sensitivity: {args.sensitivity}")
    if args.passes:
        print(f"  Passes:      {', '.join(args.passes)}")
    if args.output:
        print(f"  Output:      {args.output}")
    print("=" * 60)

    if args.dry_run:
        print("\n[DRY RUN] Would run scan with above parameters.")
        return

    # Run scan
    engine = ScanEngine(
        bbox=bbox,
        date_start=args.dates[0],
        date_end=args.dates[1],
        output_dir=args.output,
        data_dirs=args.data_dirs,
        passes=pass_overrides or None,
        lake=lake,
        label=label,
    )

    results = engine.run()

    # Summary
    print(f"\n{'=' * 60}")
    print("SCAN COMPLETE")
    print(f"{'=' * 60}")
    total = results.get("total_detections", 0)
    days = results.get("days_scanned", 0)
    print(f"  Days scanned:      {days}")
    print(f"  Total detections:  {total}")
    for pass_name, count in results.get("by_pass", {}).items():
        print(f"    {pass_name}: {count}")
    if results.get("kmz_path"):
        print(f"  KMZ: {results['kmz_path']}")
    if results.get("json_path"):
        print(f"  JSON: {results['json_path']}")
    print("=" * 60)


def _download_data(args):
    """Attempt to download missing satellite data for the requested area/dates."""
    try:
        from hls_download3 import download_hls_tiles
    except ImportError:
        try:
            from hls_download2 import download_hls_tiles
        except ImportError:
            print("⚠ No HLS downloader available — skipping download step")
            return

    lake = args.lake
    bbox = args.bbox or LAKE_PRESETS[lake]["bbox"]
    print(f"\n📡 Downloading HLS data for {lake or 'custom bbox'}...")
    try:
        download_hls_tiles(
            bbox=bbox,
            date_start=args.dates[0],
            date_end=args.dates[1],
        )
    except Exception as e:
        print(f"⚠ Download failed: {e} — continuing with existing data")


if __name__ == "__main__":
    main()
