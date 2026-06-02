#!/usr/bin/env python3
"""
DEEP WRECK VALIDATION - RAW DATA ONLY

Target: 42.9489°N, -86.9766°W (thermal detection)
Also test: Stationary Anchor #8 at 42.4647°N, -87.1082°W

Process:
1. Full band suite (B01, B04, B05, B10, B11)
2. Squeeze filters (monster + andaste profiles)
3. Report what pops, nothing else
"""

import numpy as np
from pathlib import Path
from PIL import Image
import json
from datetime import datetime

# ============================================================================
# TARGETS
# ============================================================================

TARGETS = {
    'thermal_detection': {
        'lat': 42.948873,
        'lon': -86.976619,
        'original_zscore': 5.46,
        'lock_level': 1,
        'sensors': ['thermal'],
        'tile': 'HLS.L30.T16TDN.2021198T162826.v2.0',
    },
    'stationary_anchor_8': {
        'lat': 42.4647,
        'lon': -87.1082,
        'combined_score': 18.67,
        'status': 'SWOT_PENDING',
        'tile': 'HLS.S30.T16TDN.2025244T163839.v2.0',  # Approximate
    }
}

# ============================================================================
# MONSTER PROFILE (Deep Wreck Signature)
# ============================================================================

MONSTER_THRESHOLDS = {
    'thermal_zscore_min': 3.0,
    'thermal_delta_c_min': 2.0,
    'optical_length_m_min': 100,
    'depth_m_min': 150,
    'multi_sensor_lock': True,  # Requires 2+ sensors
}

# ============================================================================
# ANDASTE PROFILE (Whaleback Signature)
# ============================================================================

ANDASTE_THRESHOLDS = {
    'thermal_zscore_min': 2.5,
    'length_m_range': (80, 100),  # 266ft whaleback spine
    'beam_m_range': (10, 15),
    'three_islands': True,  # Forward, mid, aft superstructure
    'depth_m_range': (50, 100),
}

# ============================================================================
# PROCESSING
# ============================================================================

def load_all_bands(tile_base, tile_dir):
    """Load all available bands for a tile"""
    bands = {}
    for band in ['B01', 'B04', 'B05', 'B10', 'B11']:
        tif_path = tile_dir / f"{tile_base}.{band}.tif"
        if tif_path.exists():
            img = Image.open(tif_path)
            bands[band] = np.array(img, dtype=np.float32)
            print(f"  Loaded {band}: {tif_path.name} ({bands[band].shape})")
    return bands

def process_full_suite(bands):
    """Process all bands - full sensor suite"""
    results = {}
    
    # Thermal (B10, B11)
    if 'B10' in bands and 'B11' in bands:
        thermal = (bands['B10'] + bands['B11']) / 2
        thermal_mean = np.mean(thermal)
        thermal_std = np.std(thermal)
        thermal_zscore = (thermal - thermal_mean) / (thermal_std + 1e-6)
        
        results['thermal'] = {
            'mean': float(thermal_mean),
            'std': float(thermal_std),
            'max_zscore': float(np.max(np.abs(thermal_zscore))),
            'anomaly_count': int(np.sum(np.abs(thermal_zscore) > 2.5)),
            'zscore_map': thermal_zscore
        }
    
    # Optical/NIR (B04, B05)
    if 'B04' in bands and 'B05' in bands:
        nir_ratio = bands['B05'] / (bands['B04'] + 1e-6)
        nir_mean = np.mean(nir_ratio)
        nir_std = np.std(nir_ratio)
        nir_zscore = (nir_ratio - nir_mean) / (nir_std + 1e-6)
        
        results['optical'] = {
            'mean': float(nir_mean),
            'std': float(nir_std),
            'max_zscore': float(np.max(np.abs(nir_zscore))),
            'anomaly_count': int(np.sum(np.abs(nir_zscore) > 2.5)),
            'zscore_map': nir_zscore
        }
    
    # Coastal/Shallow (B01)
    if 'B01' in bands:
        b01 = bands['B01']
        b01_mean = np.mean(b01)
        b01_std = np.std(b01)
        b01_zscore = (b01 - b01_mean) / (b01_std + 1e-6)
        
        results['coastal'] = {
            'mean': float(b01_mean),
            'std': float(b01_std),
            'max_zscore': float(np.max(np.abs(b01_zscore))),
            'anomaly_count': int(np.sum(np.abs(b01_zscore) > 2.5)),
            'zscore_map': b01_zscore
        }
    
    return results

def squeeze_filter(results, profile='monster'):
    """Apply squeeze filters for specific wreck profiles"""
    
    if profile == 'monster':
        # Deep wreck - requires strong thermal + multi-sensor
        thermal = results.get('thermal', {})
        optical = results.get('optical', {})
        
        passes = {
            'thermal_zscore': thermal.get('max_zscore', 0) >= MONSTER_THRESHOLDS['thermal_zscore_min'],
            'optical_present': 'optical' in results,
            'multi_sensor': len(results) >= 2,
        }
        
        passes_all = all(passes.values())
        
        return {
            'profile': 'MONSTER (Deep Wreck)',
            'passes': passes,
            'passes_all': passes_all,
            'sensors_detected': list(results.keys())
        }
    
    elif profile == 'andaste':
        # Whaleback - requires thermal + specific dimensions
        thermal = results.get('thermal', {})
        
        # Estimate length from anomaly cluster (simplified)
        anomaly_pixels = np.sum(np.abs(thermal.get('zscore_map', np.zeros(1))) > ANDASTE_THRESHOLDS['thermal_zscore_min'])
        estimated_length_m = np.sqrt(anomaly_pixels) * 30  # Approximate
        
        passes = {
            'thermal_zscore': thermal.get('max_zscore', 0) >= ANDASTE_THRESHOLDS['thermal_zscore_min'],
            'length_range': ANDASTE_THRESHOLDS['length_m_range'][0] <= estimated_length_m <= ANDASTE_THRESHOLDS['length_m_range'][1],
        }
        
        return {
            'profile': 'ANDASTE (Whaleback)',
            'passes': passes,
            'passes_all': all(passes.values()),
            'estimated_length_m': float(estimated_length_m),
            'sensors_detected': list(results.keys())
        }
    
    return None

# ============================================================================
# MAIN
# ============================================================================

def main():
    print("="*70)
    print("DEEP WRECK VALIDATION - RAW DATA")
    print("="*70)
    print()
    print("NO INTERPRETATION. JUST DATA.")
    print()
    
    output_dir = Path("outputs/deep_wreck_validation")
    output_dir.mkdir(parents=True, exist_ok=True)
    
    all_results = {}
    
    for target_name, target_data in TARGETS.items():
        print("="*70)
        print(f"TARGET: {target_name.upper()}")
        print("="*70)
        print(f"  Lat: {target_data['lat']:.6f}")
        print(f"  Lon: {target_data['lon']:.6f}")
        if 'original_zscore' in target_data:
            print(f"  Original Z-Score: {target_data['original_zscore']:.2f}")
        if 'combined_score' in target_data:
            print(f"  Combined Score: {target_data['combined_score']:.2f}")
        print()
        
        # Find tile directory
        tile_base = target_data['tile']
        tile_dir = Path("wreckhunter2000/data/cache/census_raw/2021_low_water")
        if '2025' in tile_base:
            tile_dir = Path("wreckhunter2000/data/cache/census_raw/2025_rossa")
        
        print(f"[1/3] Loading all bands for {tile_base}...")
        bands = load_all_bands(tile_base, tile_dir)
        
        if not bands:
            print("  ✗ NO BANDS FOUND - Skipping")
            print()
            continue
        
        print(f"  Bands loaded: {list(bands.keys())}")
        print()
        
        print("[2/3] Processing full band suite...")
        results = process_full_suite(bands)
        
        for sensor, data in results.items():
            print(f"  {sensor.upper()}:")
            print(f"    Max Z-Score: {data['max_zscore']:.2f}")
            print(f"    Anomaly Count: {data['anomaly_count']}")
        print()
        
        print("[3/3] Applying squeeze filters...")
        
        # Monster filter
        monster_result = squeeze_filter(results, 'monster')
        print(f"  MONSTER PROFILE:")
        for check, passed in monster_result['passes'].items():
            status = "✓" if passed else "✗"
            print(f"    {status} {check}: {passed}")
        print(f"    PASSES ALL: {monster_result['passes_all']}")
        print()
        
        # Andaste filter
        andaste_result = squeeze_filter(results, 'andaste')
        print(f"  ANDASTE PROFILE:")
        for check, passed in andaste_result['passes'].items():
            status = "✓" if passed else "✗"
            print(f"    {status} {check}: {passed}")
        if 'estimated_length_m' in andaste_result:
            print(f"    Estimated Length: {andaste_result['estimated_length_m']:.1f}m")
        print(f"    PASSES ALL: {andaste_result['passes_all']}")
        print()
        
        # Save results
        all_results[target_name] = {
            'coordinates': {'lat': target_data['lat'], 'lon': target_data['lon']},
            'bands_processed': list(bands.keys()),
            'sensor_results': {k: {kk: vv for kk, vv in v.items() if kk != 'zscore_map'} 
                              for k, v in results.items()},
            'monster_profile': monster_result,
            'andaste_profile': andaste_result,
            'timestamp': datetime.now().isoformat()
        }
    
    # Write output
    output_file = output_dir / f"deep_wreck_validation_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
    with open(output_file, 'w') as f:
        json.dump(all_results, f, indent=2)
    
    print("="*70)
    print(f"RESULTS SAVED: {output_file}")
    print("="*70)

if __name__ == "__main__":
    main()
