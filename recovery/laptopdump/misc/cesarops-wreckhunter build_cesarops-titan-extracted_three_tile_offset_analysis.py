#!/usr/bin/env python3
"""
THREE-TILE OFFSET ANALYSIS

Process 3 tiles through same pipeline, look for:
1. Systematic offsets in detection locations
2. Repeated patterns across dates
3. Common false positive sources

Tiles:
1. HLS.L30.T16TDN.2021182T162824.v2.0 (July 1, 2021) - Detection at 42.9773°N, -87.5288°W
2. HLS.L30.T16TDN.2021198T162826.v2.0 (July 17, 2021) - Detection at 42.9489°N, -86.9766°W
3. HLS.S30.T16TDN.2025244T163839.v2.0 (Sept 1, 2025) - Stationary Anchor area at 42.4647°N, -87.1082°W

NO INTERPRETATION. JUST DATA.
"""

import numpy as np
from pathlib import Path
from PIL import Image
import json
from datetime import datetime
from collections import defaultdict

# ============================================================================
# TILE CONFIGURATION
# ============================================================================

TILES = {
    # Original triple lock source tile - THIS IS THE REAL ONE
    'tile_1_original_triple_lock': {
        'base': 'HLS.S30.T16TDN.2025244T163839.v2.0',
        'date': '2025-09-01',
        'satellite': 'Sentinel-2',
        'resolution_m': 10,
        'detection_lat': 42.948873,  # From triple lock output
        'detection_lon': -86.976619,
        'detection_zscore': 5.46,  # Original thermal Z-score
        'dir': 'wreckhunter2000/data/cache/census_raw/2025_rossa',
        'source_bands': ['B04', 'B05', 'B11', 'B12', 'B8A'],  # What triple lock used
    },
}

# ============================================================================
# PROCESSING
# ============================================================================

def load_bands(tile_config):
    """Load all available bands for a tile"""
    bands = {}
    tile_dir = Path(tile_config['dir'])
    
    # Use bands from config, or default set
    bands_to_load = tile_config.get('source_bands', ['B01', 'B04', 'B05', 'B10', 'B11'])
    
    for band in bands_to_load:
        tif_path = tile_dir / f"{tile_config['base']}.{band}.tif"
        if tif_path.exists():
            img = Image.open(tif_path)
            bands[band] = np.array(img, dtype=np.float32)
            print(f"  Loaded {band}: {tif_path.name} ({bands[band].shape})")
    
    return bands

def process_tile(bands, tile_name):
    """Process all bands with same logic"""
    results = {
        'tile': tile_name,
        'sensors': {},
        'anomalies': [],
        'statistics': {}
    }
    
    # Thermal (B10, B11)
    if 'B10' in bands and 'B11' in bands:
        thermal = (bands['B10'] + bands['B11']) / 2
        mean_val = float(np.mean(thermal))
        std_val = float(np.std(thermal))
        zscore = (thermal - mean_val) / (std_val + 1e-6)
        
        # Find anomalies
        anomaly_mask = np.abs(zscore) > 2.5
        anomaly_count = int(np.sum(anomaly_mask))
        
        # Find top anomalies (location in pixel coords)
        anomalies = []
        if anomaly_count > 0:
            zscore_abs = np.abs(zscore)
            top_indices = np.unravel_index(np.argsort(zscore_abs.ravel())[-10:], zscore.shape)
            top_zscores = zscore_abs[top_indices]
            
            for i in range(min(len(top_indices[0]), 10)):
                anomalies.append({
                    'pixel_y': int(top_indices[0][i]),
                    'pixel_x': int(top_indices[1][i]),
                    'zscore': float(top_zscores[i])
                })
        
        results['sensors']['thermal'] = {
            'mean': mean_val,
            'std': std_val,
            'max_zscore': float(np.max(np.abs(zscore))),
            'anomaly_count': anomaly_count,
            'anomalies': anomalies[:5]  # Top 5
        }
        results['statistics']['thermal_mean'] = mean_val
        results['statistics']['thermal_std'] = std_val
    
    # Optical/NIR (B04, B05)
    if 'B04' in bands and 'B05' in bands:
        nir_ratio = bands['B05'] / (bands['B04'] + 1e-6)
        mean_val = float(np.mean(nir_ratio))
        std_val = float(np.std(nir_ratio))
        zscore = (nir_ratio - mean_val) / (std_val + 1e-6)
        
        anomaly_mask = np.abs(zscore) > 2.5
        anomaly_count = int(np.sum(anomaly_mask))
        
        results['sensors']['optical'] = {
            'mean': mean_val,
            'std': std_val,
            'max_zscore': float(np.max(np.abs(zscore))),
            'anomaly_count': anomaly_count
        }
        results['statistics']['optical_mean'] = mean_val
        results['statistics']['optical_std'] = std_val
    
    # Coastal (B01)
    if 'B01' in bands:
        b01 = bands['B01']
        mean_val = float(np.mean(b01))
        std_val = float(np.std(b01))
        zscore = (b01 - mean_val) / (std_val + 1e-6)
        
        anomaly_mask = np.abs(zscore) > 2.5
        anomaly_count = int(np.sum(anomaly_mask))
        
        results['sensors']['coastal'] = {
            'mean': mean_val,
            'std': std_val,
            'max_zscore': float(np.max(np.abs(zscore))),
            'anomaly_count': anomaly_count
        }
        results['statistics']['coastal_mean'] = mean_val
        results['statistics']['coastal_std'] = std_val
    
    return results

def compare_tiles(all_results):
    """Compare results across tiles, look for offsets/patterns"""
    comparison = {
        'sensor_stats': defaultdict(list),
        'anomaly_counts': defaultdict(list),
        'offsets': {}
    }
    
    for tile_name, results in all_results.items():
        tile_config = TILES[tile_name]
        
        # Collect statistics
        for sensor, data in results.get('sensors', {}).items():
            comparison['sensor_stats'][sensor].append({
                'tile': tile_name,
                'date': tile_config['date'],
                'mean': data['mean'],
                'std': data['std'],
                'max_zscore': data['max_zscore']
            })
            comparison['anomaly_counts'][sensor].append({
                'tile': tile_name,
                'count': data['anomaly_count']
            })
    
    # Calculate offsets between detection locations
    tile_list = list(TILES.keys())
    for i in range(len(tile_list)):
        for j in range(i+1, len(tile_list)):
            tile1 = tile_list[i]
            tile2 = tile_list[j]
            
            lat1 = TILES[tile1]['detection_lat']
            lon1 = TILES[tile1]['detection_lon']
            lat2 = TILES[tile2]['detection_lat']
            lon2 = TILES[tile2]['detection_lon']
            
            lat_offset = lat2 - lat1
            lon_offset = lon2 - lon1
            dist_km = np.sqrt((lat_offset * 111)**2 + (lon_offset * 85)**2)  # Approximate
            
            comparison['offsets'][f'{tile1}_vs_{tile2}'] = {
                'lat_offset_deg': lat_offset,
                'lon_offset_deg': lon_offset,
                'distance_km': dist_km,
                'date_diff_days': abs((datetime.strptime(TILES[tile2]['date'], '%Y-%m-%d') - 
                                       datetime.strptime(TILES[tile1]['date'], '%Y-%m-%d')).days)
            }
    
    return comparison

# ============================================================================
# MAIN
# ============================================================================

def main():
    print("="*70)
    print("THREE-TILE OFFSET ANALYSIS")
    print("="*70)
    print()
    print("NO INTERPRETATION. JUST DATA.")
    print()
    
    output_dir = Path("outputs/three_tile_analysis")
    output_dir.mkdir(parents=True, exist_ok=True)
    
    all_results = {}
    
    for tile_name, tile_config in TILES.items():
        print("="*70)
        print(f"TILE: {tile_name}")
        print("="*70)
        print(f"  Date: {tile_config['date']}")
        print(f"  Satellite: {tile_config['satellite']}")
        print(f"  Resolution: {tile_config['resolution_m']}m")
        print(f"  Original Detection: {tile_config['detection_lat']:.6f}, {tile_config['detection_lon']:.6f}")
        print(f"  Original Z-Score: {tile_config['detection_zscore']:.2f}")
        print()
        
        print("[1/2] Loading bands...")
        bands = load_bands(tile_config)
        print(f"  Bands loaded: {list(bands.keys())}")
        print()
        
        print("[2/2] Processing...")
        results = process_tile(bands, tile_name)
        all_results[tile_name] = results
        
        for sensor, data in results.get('sensors', {}).items():
            print(f"  {sensor.upper()}:")
            print(f"    Mean: {data['mean']:.2f}")
            print(f"    Std: {data['std']:.2f}")
            print(f"    Max Z-Score: {data['max_zscore']:.2f}")
            print(f"    Anomaly Count: {data['anomaly_count']}")
        print()
    
    # Compare tiles
    print("="*70)
    print("CROSS-TILE COMPARISON")
    print("="*70)
    print()
    
    comparison = compare_tiles(all_results)
    
    print("GEOGRAPHIC OFFSETS:")
    for pair, offset_data in comparison['offsets'].items():
        print(f"  {pair}:")
        print(f"    Lat Offset: {offset_data['lat_offset_deg']:.4f}°")
        print(f"    Lon Offset: {offset_data['lon_offset_deg']:.4f}°")
        print(f"    Distance: {offset_data['distance_km']:.1f} km")
        print(f"    Date Diff: {offset_data['date_diff_days']} days")
    print()
    
    print("ANOMALY COUNTS BY SENSOR:")
    for sensor, counts in comparison['anomaly_counts'].items():
        print(f"  {sensor.upper()}:")
        for c in counts:
            print(f"    {c['tile']}: {c['count']} anomalies")
    print()
    
    # Save all results
    output_file = output_dir / f"three_tile_analysis_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
    
    # Convert numpy types for JSON
    def convert(obj):
        if isinstance(obj, np.floating):
            return float(obj)
        elif isinstance(obj, np.integer):
            return int(obj)
        elif isinstance(obj, np.ndarray):
            return obj.tolist()
        elif isinstance(obj, dict):
            return {k: convert(v) for k, v in obj.items()}
        elif isinstance(obj, list):
            return [convert(v) for v in obj]
        return obj
    
    all_data = {
        'tile_results': convert(all_results),
        'comparison': convert(comparison),
        'timestamp': datetime.now().isoformat()
    }
    
    with open(output_file, 'w') as f:
        json.dump(all_data, f, indent=2)
    
    print("="*70)
    print(f"RESULTS SAVED: {output_file}")
    print("="*70)

if __name__ == "__main__":
    main()
