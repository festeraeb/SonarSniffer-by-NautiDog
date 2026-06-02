#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Daily Satellite Data Pull

Pulls data from all satellites for specified date range:
1. NDBC Buoy (weather) - Always available
2. SWOT SSH - Sparse (21-day repeat)
3. ICESat-2 ATL13 - Very sparse (91-day repeat)
4. Landsat-8/9 Thermal - Moderate (16-day repeat)
5. Sentinel-1 SAR - Good (6-day repeat)
6. Sentinel-2 Optical - Excellent (5-day repeat)

Usage:
    python daily_satellite_pull.py --days 1
    python daily_satellite_pull.py --start 2025-05-15 --end 2025-05-16
"""

import argparse
import json
import sys
from datetime import datetime, timedelta
from pathlib import Path

# ============================================================================
# CONFIGURATION
# ============================================================================

# Output directories
OUTPUT_BASE = Path(__file__).parent / "wreckhunter2000" / "outputs"
OUTPUT_BASE.mkdir(parents=True, exist_ok=True)

# Lake Michigan bounding box
LAKE_BBOX = {
    'lon_min': -87.9,
    'lat_min': 41.5,
    'lon_max': -85.5,
    'lat_max': 46.0,
}

# Token paths
EARTHDATA_TOKEN = Path(__file__).parent / "wreckhunter2000" / "earthdata_token.json"
USGS_CREDS = {
    'user': None,  # Set via env var USGS_USER
    'password': None,  # Set via env var USGS_PASS
}

# ============================================================================
# IMPORT DOWNLOADERS
# ============================================================================

def import_downloader(module_path):
    """Safely import a downloader module"""
    try:
        __import__(module_path)
        return sys.modules[module_path]
    except ImportError as e:
        print(f"  ⚠ Could not import {module_path}: {e}")
        return None

# ============================================================================
# PULL FUNCTIONS
# ============================================================================

def pull_buoy_data(start_date, end_date):
    """Pull NDBC buoy data (no auth required)"""
    print(f"\n[1/6] NDBC Buoy Data ({start_date.date()} to {end_date.date()})")
    
    try:
        from wreckhunter2000.scripts.day0_fetch_ndbc_and_wind import fetch_buoy_data
        
        output_dir = OUTPUT_BASE / "buoy" / f"{start_date.strftime('%Y-%m-%d')}_{end_date.strftime('%Y-%m-%d')}"
        output_dir.mkdir(parents=True, exist_ok=True)
        
        # Fetch for buoy 45002 (Southern Lake Michigan)
        result = fetch_buoy_data(
            start_date.strftime('%Y-%m-%d'),
            end_date.strftime('%Y-%m-%d'),
            buoy_id='45002',
            output_dir=output_dir
        )
        
        if result:
            print(f"  ✓ Buoy data saved to: {output_dir}")
        else:
            print(f"  ⚠ No buoy data available")
    
    except Exception as e:
        print(f"  ✗ Error: {e}")

def pull_swot(start_date, end_date):
    """Pull SWOT SSH data (requires Earthdata token)"""
    print(f"\n[2/6] SWOT SSH ({start_date.date()} to {end_date.date()})")
    
    if not EARTHDATA_TOKEN.exists():
        print(f"  ⚠ Earthdata token not found: {EARTHDATA_TOKEN}")
        print(f"  Create token file or skip SWOT")
        return
    
    try:
        from wreckhunter2000.swot_batch_downloader import download_swot
        
        output_dir = OUTPUT_BASE / "swot_ssh" / f"{start_date.strftime('%Y-%m-%d')}_{end_date.strftime('%Y-%m-%d')}"
        output_dir.mkdir(parents=True, exist_ok=True)
        
        result = download_swot(
            start_date.strftime('%Y-%m-%d'),
            end_date.strftime('%Y-%m-%d'),
            bbox=LAKE_BBOX,
            output_dir=output_dir
        )
        
        if result and len(result) > 0:
            print(f"  ✓ Downloaded {len(result)} SWOT granules")
        else:
            print(f"  ℹ No SWOT passes for this date range (normal - 21-day repeat)")
    
    except Exception as e:
        print(f"  ✗ Error: {e}")

def pull_icesat2(start_date, end_date):
    """Pull ICESat-2 ATL13 data (requires Earthdata token)"""
    print(f"\n[3/6] ICESat-2 ATL13 ({start_date.date()} to {end_date.date()})")
    
    if not EARTHDATA_TOKEN.exists():
        print(f"  ⚠ Earthdata token not found")
        return
    
    try:
        from wreckhunter2000.icesat2_batch_downloader import download_icesat2
        
        output_dir = OUTPUT_BASE / "icesat2_atl13" / f"{start_date.strftime('%Y-%m-%d')}_{end_date.strftime('%Y-%m-%d')}"
        output_dir.mkdir(parents=True, exist_ok=True)
        
        result = download_icesat2(
            start_date.strftime('%Y-%m-%d'),
            end_date.strftime('%Y-%m-%d'),
            bbox=LAKE_BBOX,
            output_dir=output_dir
        )
        
        if result and len(result) > 0:
            print(f"  ✓ Downloaded {len(result)} ICESat-2 granules")
        else:
            print(f"  ℹ No ICESat-2 passes (normal - 91-day repeat, very sparse)")
    
    except Exception as e:
        print(f"  ✗ Error: {e}")

def pull_landsat_thermal(start_date, end_date):
    """Pull Landsat-8/9 thermal data (requires USGS credentials)"""
    print(f"\n[4/6] Landsat Thermal ({start_date.date()} to {end_date.date()})")
    
    import os
    user = os.environ.get('USGS_USER')
    password = os.environ.get('USGS_PASS')
    
    if not user or not password:
        print(f"  ⚠ USGS credentials not set (USGS_USER, USGS_PASS)")
        print(f"  Skipping Landsat download")
        return
    
    try:
        from wreckhunter2000.landsat_thermal_fetcher import download_landsat_thermal
        
        output_dir = OUTPUT_BASE / "landsat_thermal" / f"{start_date.strftime('%Y-%m-%d')}_{end_date.strftime('%Y-%m-%d')}"
        output_dir.mkdir(parents=True, exist_ok=True)
        
        result = download_landsat_thermal(
            start_date.strftime('%Y-%m-%d'),
            end_date.strftime('%Y-%m-%d'),
            bbox=LAKE_BBOX,
            output_dir=output_dir
        )
        
        if result and len(result) > 0:
            print(f"  ✓ Downloaded {len(result)} Landsat scenes")
        else:
            print(f"  ℹ No Landsat passes (16-day repeat)")
    
    except Exception as e:
        print(f"  ✗ Error: {e}")

def pull_sentinel1_sar(start_date, end_date):
    """Pull Sentinel-1 SAR data (requires Copernicus account)"""
    print(f"\n[5/6] Sentinel-1 SAR ({start_date.date()} to {end_date.date()})")
    
    try:
        from wreckhunter2000.sar_stac_query import download_sentinel1
        
        output_dir = OUTPUT_BASE / "sentinel1_sar" / f"{start_date.strftime('%Y-%m-%d')}_{end_date.strftime('%Y-%m-%d')}"
        output_dir.mkdir(parents=True, exist_ok=True)
        
        result = download_sentinel1(
            start_date.strftime('%Y-%m-%d'),
            end_date.strftime('%Y-%m-%d'),
            bbox=LAKE_BBOX,
            output_dir=output_dir
        )
        
        if result and len(result) > 0:
            print(f"  ✓ Downloaded {len(result)} SAR scenes")
        else:
            print(f"  ℹ No SAR passes (6-day repeat)")
    
    except Exception as e:
        print(f"  ✗ Error: {e}")

def pull_sentinel2(start_date, end_date):
    """Pull Sentinel-2 optical data (requires Copernicus account)"""
    print(f"\n[6/6] Sentinel-2 Optical ({start_date.date()} to {end_date.date()})")
    
    try:
        from tools.downloaders.landsat_downloader import download_sentinel2
        
        output_dir = OUTPUT_BASE / "sentinel2_optical" / f"{start_date.strftime('%Y-%m-%d')}_{end_date.strftime('%Y-%m-%d')}"
        output_dir.mkdir(parents=True, exist_ok=True)
        
        result = download_sentinel2(
            start_date.strftime('%Y-%m-%d'),
            end_date.strftime('%Y-%m-%d'),
            bbox=LAKE_BBOX,
            output_dir=output_dir
        )
        
        if result and len(result) > 0:
            print(f"  ✓ Downloaded {len(result)} Sentinel-2 scenes")
        else:
            print(f"  ℹ No Sentinel-2 passes (5-day repeat)")
    
    except Exception as e:
        print(f"  ✗ Error: {e}")

# ============================================================================
# MAIN
# ============================================================================

def main():
    parser = argparse.ArgumentParser(description='Daily Satellite Data Pull')
    
    # Date range
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--days', type=int, default=1, help='Number of days to pull (default: 1)')
    group.add_argument('--start', type=str, help='Start date (YYYY-MM-DD)')
    group.add_argument('--end', type=str, help='End date (YYYY-MM-DD)')
    
    # Lakes
    parser.add_argument('--lakes', nargs='+', default=['MICHIGAN'], help='Lakes to pull')
    
    # Skip options
    parser.add_argument('--skip-swot', action='store_true', help='Skip SWOT')
    parser.add_argument('--skip-icesat2', action='store_true', help='Skip ICESat-2')
    parser.add_argument('--skip-landsat', action='store_true', help='Skip Landsat')
    parser.add_argument('--skip-sar', action='store_true', help='Skip SAR')
    parser.add_argument('--skip-sentinel2', action='store_true', help='Skip Sentinel-2')
    
    # Full spectrum mode
    parser.add_argument('--full-spectrum', action='store_true', 
                        help='Pull ALL bands (not just primary detection bands)')
    
    args = parser.parse_args()
    
    # Calculate date range
    if args.start and args.end:
        start_date = datetime.strptime(args.start, '%Y-%m-%d')
        end_date = datetime.strptime(args.end, '%Y-%m-%d')
    else:
        end_date = datetime.now()
        start_date = end_date - timedelta(days=args.days)
    
    print("="*70)
    print("DAILY SATELLITE DATA PULL")
    print("="*70)
    print(f"Date Range: {start_date.date()} to {end_date.date()}")
    print(f"Lakes: {', '.join(args.lakes)}")
    print("="*70)
    
    # Pull data
    pull_buoy_data(start_date, end_date)
    
    if not args.skip_swot:
        pull_swot(start_date, end_date)
    
    if not args.skip_icesat2:
        pull_icesat2(start_date, end_date)
    
    if not args.skip_landsat:
        pull_landsat_thermal(start_date, end_date)
    
    if not args.skip_sar:
        pull_sentinel1_sar(start_date, end_date)
    
    if not args.skip_sentinel2:
        pull_sentinel2(start_date, end_date)
    
    # Summary
    print("\n" + "="*70)
    print("PULL COMPLETE")
    print("="*70)
    print(f"Data saved to: {OUTPUT_BASE.absolute()}")
    print("="*70)

if __name__ == "__main__":
    main()
