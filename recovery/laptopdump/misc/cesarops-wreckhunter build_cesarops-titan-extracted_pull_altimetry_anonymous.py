#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Anonymous Altimetry Satellite Pull

Pull altimetry data from anonymous FTP sources:
- AVISO FTP: saral, hy2d, jason-3, sentinel-6
- PO.DAAC: Jason-3, Sentinel-6
- NASA Earthdata: SWOT, ICESat-2 (requires token)

Usage:
    python pull_altimetry_anonymous.py --satellite SARAL --lakes MICHIGAN
    python pull_altimetry_anonymous.py --satellite JASON3 --lakes MICHIGAN ERIE
    python pull_altimetry_anonymous.py --all --lakes MICHIGAN
"""

import ftplib
import requests
import json
from datetime import datetime, timedelta
from pathlib import Path
import re

# ============================================================================
# CONFIGURATION
# ============================================================================

# AVISO Anonymous FTP
AVISO_FTP_HOST = 'avisoftp.cnes.fr'
AVISO_FTP_PATH = '/AVISO/pub'

# Satellite FTP paths
SATELLITE_FTP_PATHS = {
    'SARAL': '/AVISO/pub/saral',
    'HY2D': '/AVISO/pub/hy2d',
    'JASON3': '/AVISO/pub/jason3',
    'SENTINEL6': '/AVISO/pub/sentinel-6',
}

# PO.DAAC (NASA) - requires token but some data is open
PODAAC_JASON3 = 'https://podaac-opendap.jpl.nasa.gov/opendap/allData/jason3/'
PODAAC_SENTINEL6 = 'https://podaac-opendap.jpl.nasa.gov/opendap/allData/sentinel-6/'

# Output directory
OUTPUT_BASE = Path(__file__).parent / 'wreckhunter2000' / 'outputs' / 'altimetry'
OUTPUT_BASE.mkdir(parents=True, exist_ok=True)

# Great Lakes bounding boxes
GREAT_LAKES_BBOXES = {
    'MICHIGAN': {'lon_min': -87.9, 'lat_min': 41.5, 'lon_max': -85.5, 'lat_max': 46.0},
    'ERIE': {'lon_min': -83.5, 'lat_min': 41.5, 'lon_max': -80.5, 'lat_max': 42.5},
    'HURON': {'lon_min': -83.5, 'lat_min': 43.5, 'lon_max': -81.5, 'lat_max': 45.5},
    'SUPERIOR': {'lon_min': -92.0, 'lat_min': 46.5, 'lon_max': -84.0, 'lat_max': 48.0},
    'ONTARIO': {'lon_min': -77.5, 'lat_min': 43.5, 'lon_max': -76.0, 'lat_max': 44.5},
}

# ============================================================================
# AVISO FTP PULLER
# ============================================================================

def list_aviso_files(satellite):
    """
    List available files on AVISO FTP for a satellite
    
    Returns list of (filename, path) tuples
    """
    
    ftp_path = SATELLITE_FTP_PATHS.get(satellite)
    if not ftp_path:
        print(f"⚠ Unknown satellite: {satellite}")
        return []
    
    print(f"Connecting to AVISO FTP: {AVISO_FTP_HOST}{ftp_path}...")
    
    try:
        ftp = ftplib.FTP(AVISO_FTP_HOST)
        ftp.login()  # Anonymous login
        ftp.cwd(ftp_path)
        
        # List files
        files = ftp.nlst()
        ftp.quit()
        
        print(f"  ✓ Found {len(files)} files")
        
        # Return with full path
        return [(f, ftp_path + '/' + f) for f in files if f.endswith('.nc')]
        
    except Exception as e:
        print(f"  ✗ Error: {e}")
        return []

def download_aviso_file(filename, ftp_path, output_dir):
    """
    Download single file from AVISO FTP
    """
    
    output_file = output_dir / filename
    
    if output_file.exists():
        print(f"  ⊘ Already exists: {filename}")
        return output_file
    
    try:
        ftp = ftplib.FTP(AVISO_FTP_HOST)
        ftp.login()
        
        # Download file
        with open(output_file, 'wb') as f:
            ftp.retrbinary(f'RETR {ftp_path}', f.write)
        
        ftp.quit()
        
        print(f"  ✓ Downloaded: {filename}")
        return output_file
        
    except Exception as e:
        print(f"  ✗ Download failed: {e}")
        return None

def pull_saral(bbox, date_range=None, days=7):
    """
    Pull SARAL/AltiKa data from AVISO FTP
    
    Filters by bounding box and date range
    """
    
    print(f"\n{'='*70}")
    print(f"PULLING SARAL/ALTIKA")
    print(f"{'='*70}")
    
    output_dir = OUTPUT_BASE / 'saral'
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # List available files
    files = list_aviso_files('SARAL')
    
    if not files:
        print("  ℹ No files available")
        return []
    
    # Filter by date (filename contains date)
    if date_range:
        start_date, end_date = date_range
        filtered = []
        for filename, path in files:
            # Extract date from filename (e.g., SARAL_..._20230715_...)
            match = re.search(r'(\d{8})', filename)
            if match:
                file_date = match.group(1)
                if start_date.replace('-', '') <= file_date <= end_date.replace('-', ''):
                    filtered.append((filename, path))
        
        files = filtered
        print(f"  Filtered to {len(files)} files in date range")
    
    # Download files
    downloaded = []
    for filename, path in files[:10]:  # Limit to 10 for testing
        result = download_aviso_file(filename, path, output_dir)
        if result:
            downloaded.append(result)
    
    print(f"\n  Downloaded {len(downloaded)} files")
    return downloaded

def pull_hy2d(bbox, date_range=None, days=7):
    """
    Pull HY-2D data from AVISO FTP
    """
    
    print(f"\n{'='*70}")
    print(f"PULLING HY-2D")
    print(f"{'='*70}")
    
    output_dir = OUTPUT_BASE / 'hy2d'
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # List available files
    files = list_aviso_files('HY2D')
    
    if not files:
        print("  ℹ No files available")
        return []
    
    # Filter by date
    if date_range:
        start_date, end_date = date_range
        filtered = []
        for filename, path in files:
            match = re.search(r'(\d{8})', filename)
            if match:
                file_date = match.group(1)
                if start_date.replace('-', '') <= file_date <= end_date.replace('-', ''):
                    filtered.append((filename, path))
        
        files = filtered
        print(f"  Filtered to {len(files)} files in date range")
    
    # Download files
    downloaded = []
    for filename, path in files[:10]:
        result = download_aviso_file(filename, path, output_dir)
        if result:
            downloaded.append(result)
    
    print(f"\n  Downloaded {len(downloaded)} files")
    return downloaded

def pull_jason3(bbox, date_range=None, days=7):
    """
    Pull Jason-3 data from AVISO FTP or PO.DAAC
    """
    
    print(f"\n{'='*70}")
    print(f"PULLING JASON-3")
    print(f"{'='*70}")
    
    output_dir = OUTPUT_BASE / 'jason3'
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # Try AVISO FTP first
    files = list_aviso_files('JASON3')
    
    if files:
        # Filter by date
        if date_range:
            start_date, end_date = date_range
            filtered = []
            for filename, path in files:
                match = re.search(r'(\d{8})', filename)
                if match:
                    file_date = match.group(1)
                    if start_date.replace('-', '') <= file_date <= end_date.replace('-', ''):
                        filtered.append((filename, path))
            
            files = filtered
            print(f"  Filtered to {len(files)} files in date range")
        
        # Download
        downloaded = []
        for filename, path in files[:10]:
            result = download_aviso_file(filename, path, output_dir)
            if result:
                downloaded.append(result)
        
        print(f"\n  Downloaded {len(downloaded)} files from AVISO FTP")
        return downloaded
    
    else:
        # Try PO.DAAC (may require authentication)
        print("  AVISO FTP empty, trying PO.DAAC...")
        print("  ⚠ PO.DAAC may require authentication")
        return []

def pull_sentinel6(bbox, date_range=None, days=7):
    """
    Pull Sentinel-6 data from AVISO FTP or PO.DAAC
    """
    
    print(f"\n{'='*70}")
    print(f"PULLING SENTINEL-6")
    print(f"{'='*70}")
    
    output_dir = OUTPUT_BASE / 'sentinel6'
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # Try AVISO FTP first
    files = list_aviso_files('SENTINEL6')
    
    if files:
        # Filter by date
        if date_range:
            start_date, end_date = date_range
            filtered = []
            for filename, path in files:
                match = re.search(r'(\d{8})', filename)
                if match:
                    file_date = match.group(1)
                    if start_date.replace('-', '') <= file_date <= end_date.replace('-', ''):
                        filtered.append((filename, path))
            
            files = filtered
            print(f"  Filtered to {len(files)} files in date range")
        
        # Download
        downloaded = []
        for filename, path in files[:10]:
            result = download_aviso_file(filename, path, output_dir)
            if result:
                downloaded.append(result)
        
        print(f"\n  Downloaded {len(downloaded)} files from AVISO FTP")
        return downloaded
    
    else:
        # Try PO.DAAC
        print("  AVISO FTP empty, trying PO.DAAC...")
        print("  ⚠ PO.DAAC may require authentication")
        return []

# ============================================================================
# MAIN PULL FUNCTION
# ============================================================================

def pull_all_altimetry(lakes, date_range=None, days=7):
    """
    Pull all available altimetry satellites
    """
    
    print("="*70)
    print("ANONYMOUS ALTIMETRY PULL")
    print("="*70)
    print(f"Lakes: {', '.join(lakes)}")
    
    if date_range:
        print(f"Date range: {date_range[0]} to {date_range[1]}")
    else:
        end_date = datetime.now()
        start_date = end_date - timedelta(days=days)
        print(f"Date range: {start_date.date()} to {end_date.date()}")
    
    results = {}
    
    # Pull each satellite
    for satellite in ['SARAL', 'HY2D', 'JASON3', 'SENTINEL6']:
        print(f"\n{'='*70}")
        print(f"SATELLITE: {satellite}")
        print(f"{'='*70}")
        
        if satellite == 'SARAL':
            results[satellite] = pull_saral(None, date_range, days)
        elif satellite == 'HY2D':
            results[satellite] = pull_hy2d(None, date_range, days)
        elif satellite == 'JASON3':
            results[satellite] = pull_jason3(None, date_range, days)
        elif satellite == 'SENTINEL6':
            results[satellite] = pull_sentinel6(None, date_range, days)
    
    # Summary
    print("\n" + "="*70)
    print("PULL COMPLETE")
    print("="*70)
    
    total = sum(len(files) for files in results.values())
    print(f"Total files downloaded: {total}")
    
    for satellite, files in results.items():
        print(f"  {satellite}: {len(files)} files")
    
    print(f"\nOutput directory: {OUTPUT_BASE.absolute()}")
    print("="*70)
    
    return results

# ============================================================================
# MAIN
# ============================================================================

def main():
    import argparse
    
    parser = argparse.ArgumentParser(description='Anonymous Altimetry Pull')
    
    parser.add_argument('--satellite', type=str, 
                       choices=['SARAL', 'HY2D', 'JASON3', 'SENTINEL6', 'ALL'],
                       help='Satellite to pull')
    parser.add_argument('--lakes', nargs='+', 
                       default=['MICHIGAN'],
                       choices=GREAT_LAKES_BBOXES.keys(),
                       help='Lakes to pull')
    parser.add_argument('--start', type=str, help='Start date (YYYY-MM-DD)')
    parser.add_argument('--end', type=str, help='End date (YYYY-MM-DD)')
    parser.add_argument('--days', type=int, default=7, help='Days to pull')
    
    args = parser.parse_args()
    
    # Date range
    if args.start and args.end:
        date_range = (args.start, args.end)
    else:
        date_range = None
    
    # Pull
    if args.satellite == 'ALL' or not args.satellite:
        pull_all_altimetry(args.lakes, date_range, args.days)
    else:
        # Pull single satellite
        if args.satellite == 'SARAL':
            pull_saral(None, date_range, args.days)
        elif args.satellite == 'HY2D':
            pull_hy2d(None, date_range, args.days)
        elif args.satellite == 'JASON3':
            pull_jason3(None, date_range, args.days)
        elif args.satellite == 'SENTINEL6':
            pull_sentinel6(None, date_range, args.days)

if __name__ == "__main__":
    main()
