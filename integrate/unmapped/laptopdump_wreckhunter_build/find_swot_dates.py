#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Find SWOT Pass Dates Over Lake Michigan

Queries NASA CMR for all SWOT granules over Lake Michigan.
Returns list of dates for prioritized pulling.

Usage:
    python find_swot_dates.py --start 2023-07-01 --end 2026-12-31
"""

import json
import requests
from datetime import datetime
from pathlib import Path

# ============================================================================
# CONFIGURATION
# ============================================================================

# Lake Michigan bounding box
LAKE_MICHIGAN_BBOX = {
    'lon_min': -87.9,
    'lat_min': 41.5,
    'lon_max': -85.5,
    'lat_max': 46.0,
}

# NASA CMR API
CMR_BASE = 'https://cmr.earthdata.nasa.gov/search/granules.json'
SWOT_PRODUCT = 'SWOT_L2_LR_SSH_2.0'

# Token path
TOKEN_PATH = Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json')

# Output
OUTPUT_FILE = Path(__file__).parent / 'swot_pass_dates.json'

# ============================================================================
# FUNCTIONS
# ============================================================================

def load_earthdata_token():
    """Load Earthdata token"""
    if not TOKEN_PATH.exists():
        print(f"⚠ Token not found: {TOKEN_PATH}")
        return None
    
    with open(TOKEN_PATH, 'r') as f:
        token = json.load(f).get('earthdata_token')
    
    return token

def query_swot_dates(start_date, end_date, token):
    """
    Query NASA CMR for SWOT granules over Lake Michigan
    
    Returns list of unique dates
    """
    
    headers = {
        'Accept': 'application/json',
        'Authorization': f'Bearer {token}',
    }
    
    # Build query
    params = {
        'short_name': SWOT_PRODUCT,
        'bounding_box': f"{LAKE_MICHIGAN_BBOX['lon_min']},{LAKE_MICHIGAN_BBOX['lat_min']},{LAKE_MICHIGAN_BBOX['lon_max']},{LAKE_MICHIGAN_BBOX['lat_max']}",
        'temporal': f"{start_date},{end_date}",
        'page_size': 2000,  # Max per page
    }
    
    print(f"Querying SWOT passes from {start_date} to {end_date}...")
    print(f"  BBOX: {LAKE_MICHIGAN_BBOX}")
    
    # Query CMR
    response = requests.get(CMR_BASE, headers=headers, params=params)
    
    if response.status_code != 200:
        print(f"  ✗ Error: {response.status_code}")
        print(f"  {response.text[:200]}")
        return []
    
    # Parse results
    data = response.json()
    
    if 'feed' not in data or 'entry' not in data['feed']:
        print(f"  ℹ No SWOT passes found")
        return []
    
    entries = data['feed']['entry']
    
    # Extract unique dates
    dates = set()
    for entry in entries:
        # Parse time_start
        time_start = entry.get('time_start', '')
        if time_start:
            date_str = time_start[:10]  # YYYY-MM-DD
            dates.add(date_str)
    
    # Sort dates
    sorted_dates = sorted(list(dates))
    
    print(f"  ✓ Found {len(sorted_dates)} unique dates with SWOT coverage")
    
    return sorted_dates

def save_dates(dates, output_file):
    """Save dates to JSON file"""
    
    output = {
        'query_date': datetime.now().isoformat(),
        'bbox': LAKE_MICHIGAN_BBOX,
        'product': SWOT_PRODUCT,
        'total_dates': len(dates),
        'dates': dates,
        'date_ranges': []
    }
    
    # Group into consecutive ranges
    if dates:
        current_range = {'start': dates[0], 'end': dates[0]}
        
        for i in range(1, len(dates)):
            prev = datetime.strptime(dates[i-1], '%Y-%m-%d')
            curr = datetime.strptime(dates[i], '%Y-%m-%d')
            
            if (curr - prev).days <= 3:  # Within 3 days = same range
                current_range['end'] = dates[i]
            else:
                output['date_ranges'].append(current_range)
                current_range = {'start': dates[i], 'end': dates[i]}
        
        output['date_ranges'].append(current_range)
    
    with open(output_file, 'w') as f:
        json.dump(output, f, indent=2)
    
    print(f"\n✓ Saved to: {output_file}")
    
    return output

def print_summary(output):
    """Print summary of SWOT passes"""
    
    print("\n" + "="*70)
    print("SWOT PASS SUMMARY")
    print("="*70)
    print(f"Total dates with coverage: {output['total_dates']}")
    print(f"Date range: {output['date_ranges'][0]['start']} to {output['date_ranges'][-1]['end']}")
    print()
    
    print("Major pass clusters:")
    for i, range_info in enumerate(output['date_ranges'][:10], 1):  # Show first 10
        start = datetime.strptime(range_info['start'], '%Y-%m-%d')
        end = datetime.strptime(range_info['end'], '%Y-%m-%d')
        days = (end - start).days + 1
        print(f"  {i:2d}. {range_info['start']} to {range_info['end']} ({days} days)")
    
    if len(output['date_ranges']) > 10:
        print(f"  ... and {len(output['date_ranges']) - 10} more clusters")
    
    print("="*70)

# ============================================================================
# MAIN
# ============================================================================

def main():
    import argparse
    
    parser = argparse.ArgumentParser(description='Find SWOT pass dates over Lake Michigan')
    parser.add_argument('--start', type=str, default='2023-07-01', help='Start date (YYYY-MM-DD)')
    parser.add_argument('--end', type=str, default='2026-12-31', help='End date (YYYY-MM-DD)')
    parser.add_argument('--output', type=str, default=str(OUTPUT_FILE), help='Output JSON file')
    
    args = parser.parse_args()
    
    print("="*70)
    print("SWOT PASS DATE FINDER")
    print("="*70)
    
    # Load token
    token = load_earthdata_token()
    if not token:
        print("✗ Cannot proceed without Earthdata token")
        return
    
    print(f"✓ Token loaded")
    
    # Query dates
    dates = query_swot_dates(args.start, args.end, token)
    
    if not dates:
        print("\n✗ No SWOT passes found")
        return
    
    # Save and summarize
    output = save_dates(dates, args.output)
    print_summary(output)
    
    print(f"\nNext step:")
    print(f"  python prioritized_satellite_pull.py --swot-dates {args.output}")

if __name__ == "__main__":
    main()
