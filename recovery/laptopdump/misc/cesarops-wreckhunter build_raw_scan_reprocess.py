"""Raw reprocess orchestrator: download missing Sentinel-2 tiles and run hard_pixel_audit

Usage:
  python raw_scan_reprocess.py --tiles 16TDN,16TET --year-window rossa

This script is intended to force a fresh run from raw TIFFs and avoid simulated candidate database only.
"""

import argparse
import os
import sys
import subprocess
from pathlib import Path

# Add root and wreckhunter2000 module path for imports
ROOT = Path(__file__).resolve().parent
os.chdir(ROOT)


def main():
    parser = argparse.ArgumentParser(description='Reprocess raw lake scan with real TIFF source')
    parser.add_argument('--tiles', default='16TDN', help='Comma-separated MGRS tiles (e.g., 16TDN,16TET)')
    parser.add_argument('--year-window', default='rossa', choices=['rossa', 'baseline'], help='Year window for NASA fallback')
    parser.add_argument('--mode', default='all', choices=['fetch-only', 'audit-only', 'all'],
                        help='Pipeline mode: fetch-only, audit-only, or all (default).')
    args = parser.parse_args()

    tile_list = [t.strip().upper() for t in args.tiles.split(',') if t.strip()]

    # Fetch step (if required)
    if args.mode in ['fetch-only', 'all']:
        from wreckhunter2000.data_fetcher_scavenger import ensure_tiles, TARGET_SCENES

        scenes = [s for s in TARGET_SCENES if s['mgrs_tile'].upper() in tile_list]
        if not scenes:
            raise ValueError(f'No configured scenes found for tiles: {tile_list}')

        print('[*] Ensuring Sentinel-2 bands (raw TIFFs) are present')
        download_results = ensure_tiles(year_window=args.year_window, scene_list=scenes)

        for lake, bands in download_results.items():
            print(f'[+] {lake} bands available: {len(bands)}')

        print('[+] Fetch step completed.')

    # Audit-only path does not require fetch step
    if args.mode in ['audit-only', 'all']:
        print('[*] Running hard_pixel_audit.py (real satellite TIFF pipeline)')
        subprocess.run([sys.executable, 'hard_pixel_audit.py'], check=True)
        print('[+] Audit step completed.')

    print('[+] raw scan reprocess complete. Check wreckhunter2000/cesarops-search/outputs for final reports and kml files.')


if __name__ == '__main__':
    import sys
    main()
