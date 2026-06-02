"""
swot_andaste_cluster_analysis.py

Extract SWOT Ka-band radar SSH (Sea Surface Height) anomalies
specifically for the ANDASTE whaleback freighter cluster and
associated high-confidence signatures in the Lake Michigan corridor.

TARGETS (Andaste Cluster):
  - Andaste Main Hull (Target #1): 42.4729°N, -87.0970°W
  - Broken Section (Target #4):    42.4675°N, -87.0813°W  
  - High-confidence stationary anchors (score > 15)
  - New arrivals with score > 9.0

EXPECTED: Large steel hull creates >1cm positive SSH anomaly
          (water surface mound from mass displacement)
"""

import json
import sqlite3
import sys
from datetime import datetime
from pathlib import Path
import glob

try:
    import netCDF4 as nc
    HAS_NETCDF = True
except ImportError:
    HAS_NETCDF = False
    print("[!] netCDF4 not installed: pip install netCDF4")

import numpy as np

# ── Configuration ─────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
DB_PATH = REPO / 'LAKE_MICHIGAN_CENSUS_2026.db'
SWOT_OUTPUT_DIR = REPO / 'outputs' / 'swot_ssh'
OUTPUT_JSON = REPO / 'outputs' / 'calibration' / 'andaste_swot_cluster_analysis.json'

# Andaste cluster coordinates
ANCASTE_CLUSTER = {
    'ANDASTE_MAIN': {
        'name': 'Andaste Main Hull (Target #1)',
        'lat': 42.4729,
        'lon': -87.0970,
        'type': 'whaleback_steel_hull',
        'expected_anomaly_m': 0.015,  # ~1.5cm expected for 300ft steel hull
    },
    'ANDASTE_BROKEN': {
        'name': 'Andaste Broken Section (Target #4)',
        'lat': 42.4675,
        'lon': -87.0813,
        'type': 'broken_hull_section',
        'expected_anomaly_m': 0.008,  # ~8mm for smaller section
    },
}

# High-confidence corridor targets to also check
CORRIDOR_TARGETS = [
    {'name': 'Anchor-1 (score 18.67)', 'lat': 42.464696, 'lon': -87.108232, 'score': 18.67},
    {'name': 'Anchor-2 (score 16.47)', 'lat': 42.465058, 'lon': -87.085549, 'score': 16.47},
    {'name': 'Anchor-3 (score 16.44)', 'lat': 42.470330, 'lon': -87.098963, 'score': 16.44},
    {'name': 'Anchor-4 (score 16.09)', 'lat': 42.460976, 'lon': -87.091341, 'score': 16.09},
    {'name': 'New Arrival #1', 'lat': 42.472861, 'lon': -87.096951, 'score': 10.0},
    {'name': 'New Arrival #2', 'lat': 42.462289, 'lon': -87.102561, 'score': 10.0},
    {'name': 'New Arrival #3', 'lat': 42.467530, 'lon': -87.081341, 'score': 10.0},
]

# Search radius for matching SWOT nadir points (meters)
SEARCH_RADIUS_M = 5000  # 5km - SWOT LR swath is ~10km half-width

SSH_ANOMALY_THRESH_M = 0.01  # 1cm threshold for significant anomaly

# ── Helpers ───────────────────────────────────────────────────────────────────

def haversine_m(lat1, lon1, lat2, lon2) -> float:
    """Calculate distance between two lat/lon points in meters"""
    R = 6_371_000.0
    phi1, phi2 = np.radians(lat1), np.radians(lat2)
    dphi = np.radians(lat2 - lat1)
    dlam = np.radians(lon2 - lon1)
    a = np.sin(dphi/2)**2 + np.cos(phi1)*np.cos(phi2)*np.sin(dlam/2)**2
    return R * 2 * np.arcsin(np.sqrt(a))


def load_swot_netcdf_files() -> list[Path]:
    """Find all downloaded SWOT NetCDF files"""
    nc_files = list(SWOT_OUTPUT_DIR.glob('*.nc'))
    
    # Filter for Expert product only (has full geophysical corrections)
    expert_files = [f for f in nc_files if 'Expert' in f.name or '_LR_SSH_' in f.name]
    
    if not expert_files:
        # Try all .nc files if no expert-filtered files found
        return nc_files
    
    return expert_files


def extract_ssh_at_coordinate(nc_path: Path, target_lat: float, target_lon: float, 
                              search_radius_m: float = SEARCH_RADIUS_M) -> dict:
    """
    Extract SSH anomaly at nearest nadir point to target coordinate.
    
    Returns dict with:
      - ssh_anomaly_m: SSH anomaly in meters (or None if not found)
      - distance_m: distance to nearest SWOT nadir point
      - pass_time: time of SWOT pass
      - quality: quality flag
    """
    if not HAS_NETCDF:
        return {'error': 'netCDF4 not installed'}
    
    try:
        dataset = nc.Dataset(nc_path, 'r')
        
        # Extract variables
        lats = dataset.variables['latitude'][:]
        lons = dataset.variables['longitude'][:]
        
        # SSH anomaly variable (may be 'ssha' or 'ssh')
        ssha_var = None
        for var_name in ['ssha', 'ssh', 'sea_surface_height_anomaly']:
            if var_name in dataset.variables:
                ssha_var = var_name
                break
        
        if ssha_var is None:
            dataset.close()
            return {'error': f'No SSH variable found in {nc_path.name}'}
        
        ssha = dataset.variables[ssha_var][:]
        
        # Quality flag
        quality = dataset.variables.get('quality_flag', None)
        if quality is not None:
            quality = quality[:]
        
        # Time variable
        time_var = None
        for var_name in ['time', 'UTC_time', 'observation_time']:
            if var_name in dataset.variables:
                time_var = var_name
                break
        
        dataset.close()
        
        # Find nearest nadir point within search radius
        min_dist = float('inf')
        best_idx = None
        
        for i in range(len(lats)):
            dist = haversine_m(target_lat, target_lon, float(lats[i]), float(lons[i]))
            if dist < min_dist:
                min_dist = dist
                best_idx = i
        
        if best_idx is None or min_dist > search_radius_m:
            return {
                'ssh_anomaly_m': None,
                'distance_m': min_dist,
                'status': 'NO_COVERAGE',
                'message': f'Nearest SWOT point {min_dist/1000:.1f}km away (>{search_radius_m/1000}km threshold)'
            }
        
        # Extract SSH anomaly at nearest point
        ssh_val = float(ssha[best_idx])
        
        # Check quality
        qual_ok = True
        if quality is not None:
            qual_ok = quality[best_idx] == 0
        
        # Get time if available
        pass_time = None
        if time_var:
            try:
                time_val = dataset.variables[time_var][best_idx]
                pass_time = str(time_val)
            except:
                pass
        
        return {
            'ssh_anomaly_m': ssh_val if qual_ok else None,
            'distance_m': min_dist,
            'pass_time': pass_time,
            'quality_ok': qual_ok,
            'status': 'FOUND' if qual_ok else 'LOW_QUALITY',
        }
        
    except Exception as e:
        return {'error': str(e)}


def analyze_cluster() -> dict:
    """Main analysis function"""
    print('='*80)
    print('SWOT SSH ANOMALY ANALYSIS - ANDASTE CLUSTER')
    print('='*80)
    print()
    
    # Find SWOT files
    swot_files = load_swot_netcdf_files()
    print(f'SWOT NetCDF files found: {len(swot_files)}')
    
    if not swot_files:
        print('[!] No SWOT NetCDF files found in outputs/swot_ssh/')
        print('    Run swot_batch_downloader.py first')
        return {'error': 'No SWOT data'}
    
    print()
    
    # Results storage
    results = {
        'analysis_date': datetime.now().isoformat(),
        'swot_files_processed': len(swot_files),
        'targets': {}
    }
    
    # Analyze Andaste primary targets
    print('ANDASTE PRIMARY TARGETS:')
    print('-'*80)
    
    for target_key, target_data in ANCASTE_CLUSTER.items():
        print(f"\n{target_data['name']}:")
        print(f"  Coordinates: {target_data['lat']:.6f}N, {target_data['lon']:.6f}W")
        print(f"  Expected anomaly: {target_data['expected_anomaly_m']*1000:.1f}mm ({target_data['type']})")
        print()
        
        # Check each SWOT pass
        ssh_values = []
        coverage_count = 0
        
        for nc_file in swot_files[:20]:  # Sample first 20 files for speed
            result = extract_ssh_at_coordinate(
                nc_file, 
                target_data['lat'], 
                target_data['lon']
            )
            
            if result.get('status') == 'FOUND' and result.get('ssh_anomaly_m') is not None:
                ssh_values.append(result['ssh_anomaly_m'])
                coverage_count += 1
        
        if ssh_values:
            mean_ssh = np.mean(ssh_values)
            std_ssh = np.std(ssh_values)
            max_ssh = np.max(ssh_values)
            
            anomaly_detected = mean_ssh > SSH_ANOMALY_THRESH_M
            
            print(f"  SWOT Coverage: {coverage_count}/{len(swot_files[:20])} passes")
            print(f"  Mean SSH Anomaly: {mean_ssh*1000:.2f}mm ± {std_ssh*1000:.2f}mm")
            print(f"  Max SSH Anomaly: {max_ssh*1000:.2f}mm")
            print(f"  Status: {'✅ ANOMALY DETECTED' if anomaly_detected else '⚠️ NO SIGNIFICANT ANOMALY'}")
            
            results['targets'][target_key] = {
                'name': target_data['name'],
                'coordinates': {'lat': target_data['lat'], 'lon': target_data['lon']},
                'expected_anomaly_m': target_data['expected_anomaly_m'],
                'swot_coverage': coverage_count,
                'mean_ssh_anomaly_m': mean_ssh,
                'std_ssh_anomaly_m': std_ssh,
                'max_ssh_anomaly_m': max_ssh,
                'anomaly_detected': anomaly_detected,
                'status': 'ANOMALY_DETECTED' if anomaly_detected else 'NO_ANOMALY'
            }
        else:
            print(f"  Status: ⏳ NO SWOT COVERAGE (swath missed target)")
            results['targets'][target_key] = {
                'name': target_data['name'],
                'coordinates': {'lat': target_data['lat'], 'lon': target_data['lon']},
                'swot_coverage': 0,
                'status': 'NO_COVERAGE'
            }
    
    print()
    print()
    
    # Analyze corridor targets
    print('CORRIDOR HIGH-CONFIDENCE TARGETS:')
    print('-'*80)
    
    for target in CORRIDOR_TARGETS:
        print(f"\n{target['name']} (score {target['score']}):")
        print(f"  Coordinates: {target['lat']:.6f}N, {target['lon']:.6f}W")
        
        ssh_values = []
        for nc_file in swot_files[:10]:  # Quick sample
            result = extract_ssh_at_coordinate(nc_file, target['lat'], target['lon'])
            if result.get('status') == 'FOUND' and result.get('ssh_anomaly_m') is not None:
                ssh_values.append(result['ssh_anomaly_m'])
        
        if ssh_values:
            mean_ssh = np.mean(ssh_values)
            anomaly_detected = mean_ssh > SSH_ANOMALY_THRESH_M
            print(f"  Mean SSH Anomaly: {mean_ssh*1000:.2f}mm")
            print(f"  Status: {'✅ ANOMALY DETECTED' if anomaly_detected else '⚠️ NO SIGNIFICANT ANOMALY'}")
            
            results['targets'][target['name']] = {
                'coordinates': {'lat': target['lat'], 'lon': target['lon']},
                'score': target['score'],
                'mean_ssh_anomaly_m': mean_ssh,
                'anomaly_detected': anomaly_detected,
            }
        else:
            print(f"  Status: ⏳ NO COVERAGE")
    
    # Save results
    OUTPUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_JSON, 'w') as f:
        json.dump(results, f, indent=2)
    
    print()
    print()
    print('='*80)
    print('SUMMARY')
    print('='*80)
    
    anomaly_count = sum(1 for t in results['targets'].values() 
                        if t.get('anomaly_detected', False))
    coverage_count = sum(1 for t in results['targets'].values() 
                         if t.get('swot_coverage', 0) > 0)
    
    print(f"Targets with SWOT coverage: {coverage_count}/{len(results['targets'])}")
    print(f"Targets with >1cm SSH anomaly: {anomaly_count}")
    print()
    print(f"Results saved: {OUTPUT_JSON}")
    print('='*80)
    
    return results


if __name__ == '__main__':
    analyze_cluster()
