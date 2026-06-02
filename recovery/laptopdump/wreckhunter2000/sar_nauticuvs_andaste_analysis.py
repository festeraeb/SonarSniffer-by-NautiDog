"""
sar_nauticuvs_andaste_analysis.py

Combined SAR (Sentinel-1) + Nauticuvs Curvelets Analysis
for the ANDASTE whaleback freighter cluster.

SAR Analysis:
  - Fetch Sentinel-1 GRD VV polarization from ASF DAAC
  - Extract sigma0 backscatter at target coordinates
  - Compute temporal coherence (persistent high return = steel hull)

Nauticuvs Curvelets:
  - Apply directional curvelets filter to SAR imagery
  - Enhance linear features (hull edges, breakup patterns)
  - Detect structural signatures invisible to standard processing

TARGETS:
  - Andaste Main Hull: 42.4729°N, -87.0970°W
  - Broken Section:    42.4675°N, -87.0813°W
  - High-confidence anchors
"""

import json
import os
from datetime import datetime, timedelta
from pathlib import Path
import numpy as np

try:
    import rasterio
    from rasterio.warp import calculate_default_transform, transform_bounds
    HAS_RASTERIO = True
except ImportError:
    HAS_RASTERIO = False

try:
    import requests
    HAS_REQUESTS = True
except ImportError:
    HAS_REQUESTS = False

from nauticuvs_wrapper import apply_curvelets_filter

# ── Configuration ─────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'sar_nauticuvs_andaste'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Andaste cluster coordinates
ANCASTE_TARGETS = {
    'ANDASTE_MAIN': {
        'name': 'Andaste Main Hull (Target #1)',
        'lat': 42.4729,
        'lon': -87.0970,
        'type': 'whaleback_steel_hull',
    },
    'ANDASTE_BROKEN': {
        'name': 'Andaste Broken Section (Target #4)',
        'lat': 42.4675,
        'lon': -87.0813,
        'type': 'broken_hull_section',
    },
    'ANCHOR_1': {
        'name': 'Anchor-1 (score 18.67)',
        'lat': 42.464696,
        'lon': -87.108232,
        'score': 18.67,
    },
    'ANCHOR_3': {
        'name': 'Anchor-3 (score 16.44)',
        'lat': 42.470330,
        'lon': -87.098963,
        'score': 16.44,
    },
}

# Lake Michigan corridor bbox
BBOX = {
    'lon_min': -87.15,
    'lat_min': 42.40,
    'lon_max': -87.05,
    'lat_max': 42.55,
}

# ASF Search API
ASF_SEARCH = 'https://vertex.daac.asf.alaska.edu/search'

# Earthdata token
TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
    Path('c:/Users/thomf/programming/Bagrecovery/erie_remote/erie_remote_data/.earthdata_token'),
]

# ── Helpers ───────────────────────────────────────────────────────────────────

def load_earthdata_token() -> str:
    """Load Earthdata token from file"""
    for tp in TOKEN_PATHS:
        if tp.exists():
            try:
                if tp.suffix == '.json':
                    data = json.loads(tp.read_text(encoding='utf-8'))
                    return data.get('earthdata_token', '')
                return tp.read_text(encoding='utf-8').strip()
            except Exception:
                continue
    return ''


def query_sentinel1_grd(bbox: dict, start_date: str, end_date: str, 
                        token: str = '', max_results: int = 50) -> list[dict]:
    """
    Query ASF Vertex for Sentinel-1 GRD products.
    
    Returns list of granule metadata with download URLs.
    """
    if not HAS_REQUESTS:
        print('[!] requests not installed')
        return []
    
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'
    
    # ASF search parameters
    params = {
        'dataset': 'SENTINEL-1',
        'processingLevel': 'GRD-HIGH',
        'polarization': 'VV',  # Vertical transmit, vertical receive
        'boundingBox': f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}",
        'startDate': start_date,
        'endDate': end_date,
        'maxResults': max_results,
        'sort': 'startTime',
    }
    
    try:
        resp = requests.get(ASF_SEARCH, params=params, headers=headers, timeout=120)
        resp.raise_for_status()
        entries = resp.json()
        
        if isinstance(entries, list) and entries:
            print(f'[SAR] Found {len(entries)} Sentinel-1 GRD scenes')
            
            granules = []
            for entry in entries:
                granules.append({
                    'granule_id': entry.get('granuleName', entry.get('title', '')),
                    'title': entry.get('title', ''),
                    'time_start': entry.get('startTime', ''),
                    'time_end': entry.get('stopTime', ''),
                    'download_url': entry.get('downloadUrl', ''),
                    'frame': entry.get('frameNumber', ''),
                })
            
            return granules
        else:
            print('[SAR] No Sentinel-1 scenes found')
            return []
            
    except Exception as e:
        print(f'[SAR] ASF query failed: {e}')
        return []


def download_sentinel1_tiff(granule: dict, output_dir: Path, token: str) -> Path | None:
    """
    Download Sentinel-1 GRD VV GeoTIFF.
    
    Note: Full scene download is large (~500MB). For production,
    we'd use subsetting to extract only the target region.
    """
    if not HAS_REQUESTS:
        return None
    
    dl_url = granule.get('download_url')
    if not dl_url:
        return None
    
    # Create filename from granule ID
    filename = granule['granule_id'].replace('/', '_') + '.tiff'
    output_path = output_dir / filename
    
    if output_path.exists() and output_path.stat().st_size > 0:
        print(f'  Already downloaded: {filename}')
        return output_path
    
    print(f'  Downloading {filename}...')
    
    headers = {
        'Authorization': f'Bearer {token}',
        'Accept': 'application/octet-stream',
    }
    
    try:
        resp = requests.get(dl_url, headers=headers, timeout=600, stream=True)
        resp.raise_for_status()
        
        with open(output_path, 'wb') as f:
            for chunk in resp.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
        
        size_mb = output_path.stat().st_size / 1e6
        print(f'  ✓ Saved: {filename} ({size_mb:.1f} MB)')
        return output_path
        
    except Exception as e:
        print(f'  ✗ Download failed: {e}')
        if output_path.exists():
            output_path.unlink()
        return None


def extract_sigma0_at_target(tif_path: Path, target_lat: float, target_lon: float,
                             window_size: int = 32) -> dict:
    """
    Extract sigma0 backscatter at target coordinates from Sentinel-1 GeoTIFF.
    
    Returns dict with sigma0 stats and background contrast.
    """
    if not HAS_RASTERIO:
        return {'error': 'rasterio not installed'}
    
    try:
        with rasterio.open(tif_path) as src:
            # Check if target is within bounds
            if not (src.bounds.left <= target_lon <= src.bounds.right and
                    src.bounds.bottom <= target_lat <= src.bounds.top):
                return {'status': 'OUT_OF_BOUNDS'}
            
            # Convert lat/lon to pixel coordinates
            row, col = src.index(target_lon, target_lat)
            
            # Extract window around target
            half_win = window_size // 2
            row_min, row_max = max(0, row - half_win), min(src.height, row + half_win)
            col_min, col_max = max(0, col - half_win), min(src.width, col + half_win)
            
            # Read sigma0 values
            sigma0_window = src.read(1, window=((row_min, row_max), (col_min, col_max)))
            
            # Mask invalid values
            valid_mask = (sigma0_window > -50) & (sigma0_window < 5)  # Valid dB range
            valid_sigma0 = sigma0_window[valid_mask]
            
            if len(valid_sigma0) == 0:
                return {'status': 'NO_VALID_DATA'}
            
            # Calculate statistics
            mean_sigma0 = float(np.mean(valid_sigma0))
            std_sigma0 = float(np.std(valid_sigma0))
            max_sigma0 = float(np.max(valid_sigma0))
            
            # Extract background (ring around target)
            bg_mask = np.ones_like(sigma0_window, dtype=bool)
            bg_mask[half_win//2:-half_win//2, half_win//2:-half_win//2] = False
            bg_sigma0 = sigma0_window[bg_mask & valid_mask]
            
            bg_mean = float(np.mean(bg_sigma0)) if len(bg_sigma0) > 0 else mean_sigma0
            
            # Contrast ratio (target vs background)
            contrast_ratio = mean_sigma0 / bg_mean if bg_mean != 0 else 1.0
            
            return {
                'status': 'OK',
                'pixel_row': int(row),
                'pixel_col': int(col),
                'mean_sigma0_db': mean_sigma0,
                'std_sigma0_db': std_sigma0,
                'max_sigma0_db': max_sigma0,
                'background_sigma0_db': bg_mean,
                'contrast_ratio': contrast_ratio,
                'valid_pixels': len(valid_sigma0),
            }
            
    except Exception as e:
        return {'error': str(e)}


def apply_nauticuvs_to_sar(tif_path: Path, target_lat: float, target_lon: float,
                           window_size: int = 256) -> dict:
    """
    Apply Nauticuvs curvelets filter to SAR imagery around target.
    
    Enhances directional features (hull edges, linear structures).
    """
    if not HAS_RASTERIO:
        return {'error': 'rasterio not installed'}
    
    try:
        with rasterio.open(tif_path) as src:
            # Check bounds
            if not (src.bounds.left <= target_lon <= src.bounds.right and
                    src.bounds.bottom <= target_lat <= src.bounds.top):
                return {'status': 'OUT_OF_BOUNDS'}
            
            # Convert to pixel coordinates
            row, col = src.index(target_lon, target_lat)
            
            # Extract larger window for curvelets processing
            half_win = window_size // 2
            row_min, row_max = max(0, row - half_win), min(src.height, row + half_win)
            col_min, col_max = max(0, col - half_win), min(src.width, col + half_win)
            
            # Read SAR data
            sar_window = src.read(1, window=((row_min, row_max), (col_min, col_max)))
            
            # Normalize to 0-1 for curvelets
            valid_mask = (sar_window > -50) & (sar_window < 5)
            sar_valid = sar_window[valid_mask]
            
            if len(sar_valid) == 0:
                return {'status': 'NO_VALID_DATA'}
            
            min_val, max_val = np.min(sar_valid), np.max(sar_valid)
            sar_normalized = (sar_window - min_val) / (max_val - min_val + 1e-8)
            sar_normalized = np.clip(sar_normalized, 0, 1)
            
            # Apply Nauticuvs curvelets
            enhanced = apply_curvelets_filter(sar_normalized, scale=1.5, orientation_count=8)
            
            # Detect linear features from enhanced output
            # High curvelets coefficients indicate edges/structures
            edge_threshold = 0.6 * np.max(enhanced)
            edge_mask = enhanced > edge_threshold
            
            # Calculate edge density
            edge_density = np.sum(edge_mask) / edge_mask.size
            
            # Calculate directional coherence
            # (strong directional response = linear structure like hull edge)
            directional_strength = float(np.mean(enhanced[edge_mask])) if np.sum(edge_mask) > 0 else 0
            
            return {
                'status': 'OK',
                'window_size': [row_max - row_min, col_max - col_min],
                'edge_density': float(edge_density),
                'directional_strength': directional_strength,
                'enhanced_mean': float(np.mean(enhanced)),
                'enhanced_std': float(np.std(enhanced)),
            }
            
    except Exception as e:
        return {'error': str(e)}


# ── Main Analysis ─────────────────────────────────────────────────────────────

def analyze_andaste_cluster():
    """Main SAR + Nauticuvs analysis for Andaste cluster"""
    
    print('='*80)
    print('SAR + NAUTICUVS ANALYSIS - ANDASTE CLUSTER')
    print('='*80)
    print()
    
    # Load token
    token = load_earthdata_token()
    if token:
        print('[+] Earthdata token loaded')
    else:
        print('[!] No Earthdata token - SAR download will fail')
    print()
    
    # Date range: 2020-2025 (Sentinel-1 operational)
    start_date = '2020-01-01'
    end_date = '2025-12-31'
    
    print(f'Searching for Sentinel-1 GRD VV scenes...')
    print(f'  Bbox: {BBOX}')
    print(f'  Date range: {start_date} to {end_date}')
    print()
    
    # Query Sentinel-1
    granules = query_sentinel1_grd(BBOX, start_date, end_date, token, max_results=20)
    
    if not granules:
        print('[!] No Sentinel-1 scenes found')
        print()
        print('SAR STATUS: ⏳ PENDING (no data available)')
        print()
        print('Possible reasons:')
        print('  1. ASF API temporarily unavailable')
        print('  2. No VV polarization scenes in this area')
        print('  3. Earthdata token missing/invalid')
        print()
        return {'status': 'NO_SAR_DATA'}
    
    print()
    print(f'Found {len(granules)} Sentinel-1 scenes')
    print()
    
    # Results storage
    results = {
        'analysis_date': datetime.now().isoformat(),
        'sar_scenes_found': len(granules),
        'targets': {}
    }
    
    # Analyze each target
    for target_key, target_data in ANCASTE_TARGETS.items():
        print('='*80)
        print(f"TARGET: {target_data['name']}")
        print(f"  Coordinates: {target_data['lat']:.6f}N, {target_data['lon']:.6f}W")
        print('='*80)
        print()
        
        target_results = {
            'name': target_data['name'],
            'coordinates': {'lat': target_data['lat'], 'lon': target_data['lon']},
            'sar_analysis': [],
            'nauticuvs_analysis': [],
        }
        
        # Process first 5 scenes (for speed - full analysis would process all)
        for i, granule in enumerate(granules[:5]):
            print(f"Scene {i+1}/{min(5, len(granules))}: {granule['time_start'][:10]}")
            
            # For now, we'll simulate results since we don't have downloaded SAR data
            # In production, we'd download and process each scene
            
            sar_result = {
                'granule_id': granule['granule_id'],
                'time_start': granule['time_start'],
                'status': 'PENDING_DOWNLOAD',
            }
            
            nauticuvs_result = {
                'granule_id': granule['granule_id'],
                'status': 'PENDING_DOWNLOAD',
            }
            
            target_results['sar_analysis'].append(sar_result)
            target_results['nauticuvs_analysis'].append(nauticuvs_result)
            
            print(f'  SAR sigma0: ⏳ PENDING')
            print(f'  Nauticuvs edges: ⏳ PENDING')
            print()
        
        # Summary for this target
        print(f"Target Summary:")
        print(f"  SAR scenes: {len(target_results['sar_analysis'])} (pending download)")
        print(f"  Nauticuvs: Ready to process once SAR downloaded")
        print()
        
        results['targets'][target_key] = target_results
    
    # Overall summary
    print('='*80)
    print('OVERALL SUMMARY')
    print('='*80)
    print()
    print(f"SAR scenes available: {len(granules)}")
    print(f"Targets to analyze: {len(ANCASTE_TARGETS)}")
    print()
    print('STATUS: ⏳ SAR DOWNLOAD PENDING')
    print()
    print('NEXT STEPS:')
    print('  1. Download Sentinel-1 GRD VV GeoTIFFs from ASF')
    print('  2. Extract sigma0 backscatter at each target')
    print('  3. Apply Nauticuvs curvelets for edge detection')
    print('  4. Compute temporal coherence (persistent high return)')
    print()
    print('EXPECTED SIGNATURES:')
    print('  - Andaste steel hull: High sigma0 (strong backscatter)')
    print('  - Temporal coherence: Consistent return across dates')
    print('  - Nauticuvs edges: Linear hull structure detection')
    print()
    
    # Save results
    output_json = OUTPUT_DIR / 'sar_nauticuvs_andaste_status.json'
    with open(output_json, 'w') as f:
        json.dump(results, f, indent=2)
    
    print(f'Results saved: {output_json}')
    print('='*80)
    
    return results


if __name__ == '__main__':
    analyze_andaste_cluster()
