#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Prioritized Satellite Pull - MAXIMUM FUSION MODE

Strategy:
1. Find ALL SWOT pass dates for all 5 Great Lakes
2. If SWOT covers ANY lake on a date → Pull ALL 5 lakes + ALL sensors
3. Process EACH date COMPLETELY before moving to next
4. Non-SWOT dates: Sentinel-1/2, Landsat, buoy only

Usage:
    python prioritized_pull_v2.py --lakes MICHIGAN ERIE HURON SUPERIOR ONTARIO
    python prioritized_pull_v2.py --swot-only
    python prioritized_pull_v2.py --all-dates
"""

import json
import argparse
from datetime import datetime, timedelta
from pathlib import Path

# Import the daily pull function
from daily_satellite_pull import (
    pull_buoy_data,
    pull_swot,
    pull_icesat2,
    pull_landsat_thermal,
    pull_sentinel1_sar,
    pull_sentinel2,
)

# ============================================================================
# CONFIGURATION
# ============================================================================

# All 5 Great Lakes bounding boxes
GREAT_LAKES_BBOXES = {
    'MICHIGAN': {
        'name': 'Lake Michigan',
        'bbox': {'lon_min': -87.9, 'lat_min': 41.5, 'lon_max': -85.5, 'lat_max': 46.0},
        'priority': 1,
    },
    'ERIE': {
        'name': 'Lake Erie',
        'bbox': {'lon_min': -83.5, 'lat_min': 41.5, 'lon_max': -80.5, 'lat_max': 42.5},
        'priority': 2,
    },
    'HURON': {
        'name': 'Lake Huron',
        'bbox': {'lon_min': -83.5, 'lat_min': 43.5, 'lon_max': -81.5, 'lat_max': 45.5},
        'priority': 3,
    },
    'SUPERIOR': {
        'name': 'Lake Superior',
        'bbox': {'lon_min': -92.0, 'lat_min': 46.5, 'lon_max': -84.0, 'lat_max': 48.0},
        'priority': 4,
    },
    'ONTARIO': {
        'name': 'Lake Ontario',
        'bbox': {'lon_min': -77.5, 'lat_min': 43.5, 'lon_max': -76.0, 'lat_max': 44.5},
        'priority': 5,
    },
}

# SWOT date cache file
SWOT_DATES_FILE = Path(__file__).parent / 'great_lakes_swot_dates.json'

# Output directory
OUTPUT_BASE = Path(__file__).parent / 'wreckhunter2000' / 'outputs'

# ============================================================================
# SWOT DATE FINDER (Multi-Lake)
# ============================================================================

def find_all_swot_dates(lakes, start_date='2023-07-01', end_date='2026-12-31'):
    """
    Find SWOT pass dates for all specified lakes
    
    Returns dict: {lake_id: [dates]}
    """
    
    from find_swot_dates import load_earthdata_token, query_swot_dates
    
    token = load_earthdata_token()
    if not token:
        print("⚠ No Earthdata token - skipping SWOT date query")
        return {}
    
    all_dates = {}
    
    for lake_id in lakes:
        lake = GREAT_LAKES_BBOXES[lake_id]
        print(f"\nFinding SWOT passes for {lake['name']}...")
        
        # Temporarily update the bbox in find_swot_dates
        import find_swot_dates
        original_bbox = find_swot_dates.LAKE_MICHIGAN_BBOX.copy()
        find_swot_dates.LAKE_MICHIGAN_BBOX = lake['bbox']
        
        # Query
        dates = query_swot_dates(start_date, end_date, token)
        
        # Restore original bbox
        find_swot_dates.LAKE_MICHIGAN_BBOX = original_bbox
        
        all_dates[lake_id] = dates
        print(f"  ✓ {len(dates)} dates for {lake['name']}")
    
    # Save combined dates
    combined = {
        'generated_at': datetime.now().isoformat(),
        'date_range': {'start': start_date, 'end': end_date},
        'lakes': {}
    }
    
    for lake_id, dates in all_dates.items():
        combined['lakes'][lake_id] = {
            'name': GREAT_LAKES_BBOXES[lake_id]['name'],
            'total_dates': len(dates),
            'dates': dates
        }
    
    # Find union of all dates (any lake has SWOT)
    all_swot_dates = set()
    for dates in all_dates.values():
        all_swot_dates.update(dates)
    
    combined['all_lakes_union'] = {
        'total_unique_dates': len(all_swot_dates),
        'dates': sorted(list(all_swot_dates))
    }
    
    with open(SWOT_DATES_FILE, 'w') as f:
        json.dump(combined, f, indent=2)
    
    print(f"\n✓ Saved SWOT dates to: {SWOT_DATES_FILE}")
    print(f"  Total unique SWOT dates (all lakes): {len(all_swot_dates)}")
    
    return all_dates

# ============================================================================
# PRIORITIZED PULL - MAXIMUM FUSION MODE
# ============================================================================

def prioritized_pull(lakes, swot_dates, date_range=None, days=1):
    """
    Pull satellite data with priority:
    1. SWOT dates first (all sensors, ALL 5 LAKES)
    2. Process EACH date COMPLETELY before moving to next
    3. Non-SWOT dates (Sentinel-1/2, Landsat, buoy only)
    
    Strategy: If SWOT covers ANY Great Lake → Pull ALL lakes + ALL sensors
    """
    
    print("="*70)
    print("PRIORITIZED SATELLITE PULL - MAXIMUM FUSION MODE")
    print("="*70)
    print(f"Lakes: {', '.join(lakes)}")
    print("Strategy: If SWOT on ANY lake → Pull ALL 5 lakes + ALL sensors")
    print()
    
    # Calculate date range
    if date_range:
        start_date = datetime.strptime(date_range[0], '%Y-%m-%d')
        end_date = datetime.strptime(date_range[1], '%Y-%m-%d')
    else:
        end_date = datetime.now()
        start_date = end_date - timedelta(days=days)
    
    print(f"Date range: {start_date.date()} to {end_date.date()}")
    swot_in_range = [d for d in swot_dates if start_date.strftime('%Y-%m-%d') <= d <= end_date.strftime('%Y-%m-%d')]
    print(f"SWOT dates in range: {len(swot_in_range)}")
    print()
    
    # Generate all dates in range
    all_dates = []
    current = start_date
    while current <= end_date:
        all_dates.append(current.strftime('%Y-%m-%d'))
        current += timedelta(days=1)
    
    # Separate SWOT vs non-SWOT dates
    swot_dates_in_range = sorted([d for d in swot_dates if d in all_dates])
    non_swot_dates = [d for d in all_dates if d not in swot_dates_in_range]
    
    print("="*70)
    print("PRIORITY 1: SWOT DATES (Full Spectrum - ALL 5 LAKES)")
    print("="*70)
    print(f"Dates: {len(swot_dates_in_range)}")
    print()
    for date_str in swot_dates_in_range[:5]:  # Show first 5
        print(f"  - {date_str}")
    if len(swot_dates_in_range) > 5:
        print(f"  ... and {len(swot_dates_in_range) - 5} more")
    
    print()
    print("Processing strategy:")
    print("  For EACH SWOT date:")
    print("    1. Pull ALL satellites for ALL 5 lakes")
    print("    2. Process completely (SWOT + SAR + Optical + Thermal + Buoy)")
    print("    3. Move to next date (only when current is COMPLETE)")
    print()
    print("Starting pull...")
    print()
    
    # Process each SWOT date COMPLETELY before moving to next
    for i, date_str in enumerate(swot_dates_in_range, 1):
        print(f"{'='*70}")
        print(f"DATE {i}/{len(swot_dates_in_range)}: {date_str} (SWOT PRIORITY)")
        print(f"{'='*70}")
        
        date = datetime.strptime(date_str, '%Y-%m-%d')
        
        # Pull for ALL lakes on this date
        for lake_id in lakes:
            lake = GREAT_LAKES_BBOXES[lake_id]
            print(f"\n  [{i}/{len(swot_dates_in_range)}] Lake: {lake['name']}")
            
            # SWOT (this is why we're here!)
            print(f"      → SWOT SSH...")
            pull_swot(date, date + timedelta(days=1))
            
            # ICESat-2
            print(f"      → ICESat-2 ATL13...")
            pull_icesat2(date, date + timedelta(days=1))
            
            # Landsat
            print(f"      → Landsat Thermal...")
            pull_landsat_thermal(date, date + timedelta(days=1))
            
            # Sentinel-1
            print(f"      → Sentinel-1 SAR...")
            pull_sentinel1_sar(date, date + timedelta(days=1))
            
            # Sentinel-2
            print(f"      → Sentinel-2 Optical...")
            pull_sentinel2(date, date + timedelta(days=1))
            
            # Buoy
            print(f"      → NDBC Buoy...")
            pull_buoy_data(date, date + timedelta(days=1))
        
        print(f"\n  ✓✓✓ DATE {date_str} COMPLETE ✓✓✓")
        print(f"      All {len(lakes)} lakes, all sensors processed")
        print(f"      Ready for next date...")
        print()
    
    print()
    print("="*70)
    print("PRIORITY 2: NON-SWOT DATES (Standard Monitoring)")
    print("="*70)
    print(f"Dates: {len(non_swot_dates)}")
    
    if non_swot_dates:
        print()
        print("Pulling Sentinel-1/2, Landsat, Buoy (no SWOT/ICESat-2)...")
        print()
        
        for date_str in non_swot_dates[:3]:  # Show first 3
            print(f"  - {date_str}")
        if len(non_swot_dates) > 3:
            print(f"  ... and {len(non_swot_dates) - 3} more")
        print()
        
        # For non-SWOT dates, skip SWOT and ICESat-2 (waste of time)
        for date_str in non_swot_dates:
            date = datetime.strptime(date_str, '%Y-%m-%d')
            
            # Just the essentials
            pull_buoy_data(date, date + timedelta(days=1))
            pull_sentinel1_sar(date, date + timedelta(days=1))
            pull_sentinel2(date, date + timedelta(days=1))
            pull_landsat_thermal(date, date + timedelta(days=1))
    
    print()
    print("="*70)
    print("PULL COMPLETE")
    print("="*70)
    print(f"SWOT dates processed: {len(swot_dates_in_range)}")
    print(f"  → Each date: ALL lakes, ALL sensors")
    print(f"Non-SWOT dates processed: {len(non_swot_dates)}")
    print(f"  → Sentinel-1/2, Landsat, Buoy only")
    print(f"Total dates: {len(all_dates)}")
    print("="*70)

# ============================================================================
# MAIN
# ============================================================================

def main():
    parser = argparse.ArgumentParser(description='Prioritized Satellite Pull - Maximum Fusion')
    
    # Lakes
    parser.add_argument('--lakes', nargs='+', 
                        default=['MICHIGAN', 'ERIE', 'HURON', 'SUPERIOR', 'ONTARIO'],
                        choices=GREAT_LAKES_BBOXES.keys(),
                        help='Lakes to process')
    
    # Mode
    mode_group = parser.add_mutually_exclusive_group()
    mode_group.add_argument('--swot-only', action='store_true', 
                           help='Only pull SWOT dates (maximum multi-sensor fusion)')
    mode_group.add_argument('--all-dates', action='store_true',
                           help='Pull all dates (SWOT + non-SWOT)')
    mode_group.add_argument('--find-swot-dates', action='store_true',
                           help='Only find SWOT dates, don\'t pull')
    
    # Date range
    parser.add_argument('--start', type=str, help='Start date (YYYY-MM-DD)')
    parser.add_argument('--end', type=str, help='End date (YYYY-MM-DD)')
    parser.add_argument('--days', type=int, default=7, help='Days to pull (default: 7)')
    
    # Full spectrum
    parser.add_argument('--full-spectrum', action='store_true',
                       help='Pull ALL bands (not just primary)')
    
    args = parser.parse_args()
    
    # Find SWOT dates first
    print("="*70)
    print("STEP 1: FINDING SWOT PASS DATES")
    print("="*70)
    
    start_date = args.start if args.start else '2023-07-01'
    end_date = args.end if args.end else '2026-12-31'
    
    if args.find_swot_dates:
        find_all_swot_dates(args.lakes, start_date, end_date)
        return
    
    # Load or generate SWOT dates
    if SWOT_DATES_FILE.exists():
        print(f"Loading cached SWOT dates from {SWOT_DATES_FILE}")
        with open(SWOT_DATES_FILE, 'r') as f:
            swot_data = json.load(f)
        
        # Get union of all dates
        all_swot_dates = swot_data.get('all_lakes_union', {}).get('dates', [])
    else:
        print("Finding SWOT dates for all lakes...")
        find_all_swot_dates(args.lakes, start_date, end_date)
        
        with open(SWOT_DATES_FILE, 'r') as f:
            swot_data = json.load(f)
        all_swot_dates = swot_data.get('all_lakes_union', {}).get('dates', [])
    
    print(f"✓ Found {len(all_swot_dates)} unique SWOT dates (all lakes)")
    print()
    
    # Pull based on mode
    if args.swot_only:
        print("\n" + "="*70)
        print("MODE: SWOT ONLY (Maximum Fusion)")
        print("="*70)
        prioritized_pull(args.lakes, all_swot_dates, 
                        date_range=(args.start, args.end) if args.start and args.end else None,
                        days=args.days)
    
    elif args.all_dates:
        print("\n" + "="*70)
        print("MODE: ALL DATES (Complete Dataset)")
        print("="*70)
        prioritized_pull(args.lakes, all_swot_dates,
                        date_range=(args.start, args.end) if args.start and args.end else None,
                        days=args.days)
    
    else:
        # Default: SWOT dates only for this run
        print("\n" + "="*70)
        print("MODE: DEFAULT (SWOT Priority)")
        print("="*70)
        prioritized_pull(args.lakes, all_swot_dates,
                        date_range=(args.start, args.end) if args.start and args.end else None,
                        days=args.days)

if __name__ == "__main__":
    main()
