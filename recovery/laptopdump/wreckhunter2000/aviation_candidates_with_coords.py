"""
aviation_candidates_with_coords.py

Extracts LAT/LON coordinates for the top 97 aluminum candidates.

Uses Sentinel-2 georeferencing to convert pixel row/col to lat/lon.
"""

import json
import rasterio
from pathlib import Path
from datetime import datetime

# ── Configuration ─────────────────────────────────────────────────────────────

OUTPUT_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/aviation_filter')
GPU_CHUNKED_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/gpu_chunked')
SENTINEL2_TIFF_DIR = Path('c:/Users/thomf/programming/Bagrecovery/outputs/rossa_forensic_cache')

# Load the aviation filter results
with open(OUTPUT_DIR / 'aviation_filter_full_scan.json', 'r') as f:
    aviation_data = json.load(f)

candidates = aviation_data['aluminum_candidates']
print(f'Loaded {len(candidates)} aluminum candidates')
print()

# ── Extract Coordinates ───────────────────────────────────────────────────────

def get_coordinates_for_candidate(candidate: dict) -> dict:
    """
    Convert pixel row/col to lat/lon using Sentinel-2 georeferencing.
    
    Candidate format:
    {
        'file': 'S2C_16TDN_20250916_0_L2A.B02_anomalies_gpu_chunked_*.json',
        'scale': 0,
        'direction': 0,
        'row': 4755,
        'col': 1314,
        'magnitude': 1.235,
    }
    
    Returns:
    {
        ...candidate data...,
        'lat': 42.XXXX,
        'lon': -87.XXXX,
        'band': 'B02',
    }
    """
    
    # Find corresponding Sentinel-2 TIFF file
    # Filename format: S2C_16TDN_20250916_0_L2A.B02.tif
    # Extract band from candidate filename
    filename = candidate['file']
    # Example: S2C_16TDN_20250916_0_L2A.B02_anomalies_gpu_chunked_20260324_224016.json
    parts = filename.split('.')
    band = parts[1][:3] if len(parts) > 1 else 'UNKNOWN'  # Extract 'B02' from 'B02_anomalies...'
    
    band_tiff = f'S2C_16TDN_20250916_0_L2A.{band}.tif'
    tiff_path = SENTINEL2_TIFF_DIR / band_tiff
    
    if not tiff_path.exists():
        print(f'  ⚠ TIFF not found: {band_tiff}')
        return {**candidate, 'lat': None, 'lon': None, 'band': band, 'error': 'TIFF not found'}
    
    # Open TIFF and get georeferencing
    try:
        with rasterio.open(tiff_path) as src:
            # Convert pixel row/col to lat/lon
            # Note: row/col from GPU processing may need offset adjustment
            row = candidate['row']
            col = candidate['col']
            
            # Rasterio uses (x, y) = (col, row)
            # Get coordinates in CRS units (usually UTM meters)
            x, y = src.xy(row, col)
            
            # Check if CRS is UTM (meters) and convert to lat/lon
            if src.crs.is_projected:
                from pyproj import Transformer
                transformer = Transformer.from_crs(src.crs, "EPSG:4326", always_xy=True)
                lon, lat = transformer.transform(x, y)
            else:
                lon, lat = x, y
            
            return {
                **candidate,
                'lat': lat,
                'lon': lon,
                'band': band,
                'depth_estimate_m': 'Unknown (need bathymetry)',
                'utm_x': x,
                'utm_y': y,
            }
    
    except Exception as e:
        print(f'  ⚠ Error reading {band_tiff}: {e}')
        return {**candidate, 'lat': None, 'lon': None, 'band': band, 'error': str(e)}


def main():
    """Extract coordinates for all candidates."""
    
    print('='*70)
    print('AVIATION CANDIDATES - EXTRACTING COORDINATES')
    print('='*70)
    print()
    
    candidates_with_coords = []
    
    for i, candidate in enumerate(candidates, 1):
        print(f'[{i}/{len(candidates)}] Processing {candidate["file"][:50]}...')
        
        result = get_coordinates_for_candidate(candidate)
        candidates_with_coords.append(result)
        
        if result['lat'] and result['lon']:
            print(f'  ✓ Lat: {result["lat"]:.6f}, Lon: {result["lon"]:.6f}, Band: {result["band"]}, Mag: {result["magnitude"]:.4f}')
        else:
            print(f'  ✗ Error: {result.get("error", "Unknown")}')
        print()
    
    # Sort by magnitude (highest first)
    candidates_with_coords.sort(key=lambda x: -x['magnitude'])
    
    # Save results
    output_path = OUTPUT_DIR / 'aviation_candidates_with_coords.json'
    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump({
            'extraction_date': datetime.now().isoformat(),
            'total_candidates': len(candidates_with_coords),
            'candidates_with_valid_coords': len([c for c in candidates_with_coords if c.get('lat')]),
            'candidates': candidates_with_coords,
        }, f, indent=2)
    
    print('='*70)
    print('SUMMARY')
    print('='*70)
    print(f'Total candidates: {len(candidates_with_coords)}')
    print(f'With valid coordinates: {len([c for c in candidates_with_coords if c.get("lat")])}')
    print(f'Without coordinates: {len([c for c in candidates_with_coords if not c.get("lat")])}')
    print()
    
    if candidates_with_coords:
        print('TOP 10 ALUMINUM CANDIDATES (with coordinates):')
        print()
        for i, c in enumerate(candidates_with_coords[:10], 1):
            if c.get('lat') and c.get('lon'):
                print(f'{i:2d}. Band: {c["band"]:3s} | Mag: {c["magnitude"]:.4f} | Lat: {c["lat"]:.6f} | Lon: {c["lon"]:.6f}')
            else:
                print(f'{i:2d}. Band: {c["band"]:3s} | Mag: {c["magnitude"]:.4f} | NO COORDINATES')
        print()
        print('Full results saved: ' + str(output_path))
    
    print('='*70)


if __name__ == '__main__':
    main()
