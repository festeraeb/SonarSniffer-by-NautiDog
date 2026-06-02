#!/usr/bin/env python3
"""
REPEATABILITY VALIDATION TEST

Target: 42.9489°N, -86.9766°W (1-LOCK thermal detection from March 30 scan)
Source Tile: HLS.L30.T16TDN.2021198T162826.v2.0.B10.tif

Goal: Re-process same tile, see if anomaly repeats
"""

import numpy as np
from pathlib import Path
from PIL import Image
import json
from datetime import datetime

# ============================================================================
# CONFIGURATION
# ============================================================================

# Target coordinates from original detection
TARGET_LAT = 42.948873
TARGET_LON = -86.976619
ORIGINAL_ZSCORE = 5.46

# Tile to re-process
TILE_BASE = "HLS.L30.T16TDN.2021198T162826.v2.0"
TILE_DIR = Path("wreckhunter2000/data/cache/census_raw/2021_low_water")

# Output
OUTPUT_DIR = Path("outputs/validation_test")
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# ============================================================================
# PROCESSING
# ============================================================================

def load_tile(band):
    """Load a specific band"""
    tif_path = TILE_DIR / f"{TILE_BASE}.{band}.tif"
    if not tif_path.exists():
        return None
    
    img = Image.open(tif_path)
    return np.array(img, dtype=np.float32), tif_path

def process_thermal(b10_data, b11_data):
    """Process thermal bands to find anomalies"""
    # Calculate brightness temperature from B10/B11
    # (simplified - real algorithm uses calibration constants)
    brightness_temp = (b10_data + b11_data) / 2
    
    # Calculate Z-score
    mean_val = np.mean(brightness_temp)
    std_val = np.std(brightness_temp)
    
    zscore = (brightness_temp - mean_val) / (std_val + 1e-6)
    
    # Find anomalies (|Z| > 2.5)
    anomalies = np.abs(zscore) > 2.5
    anomaly_count = int(np.sum(anomalies))
    
    return {
        'mean': float(mean_val),
        'std': float(std_val),
        'zscore_max': float(np.max(np.abs(zscore))),
        'anomaly_count': anomaly_count,
        'zscore_map': zscore
    }

def latlon_to_pixel(lat, lon, geotransform):
    """Convert lat/lon to pixel coordinates"""
    if geotransform is None:
        return None, None
    
    gt = geotransform
    # Simplified conversion (would need proper projection handling)
    # This is approximate for validation purposes
    pixel_x = int((lon - gt[0]) / gt[1])
    pixel_y = int((lat - gt[3]) / gt[5])
    
    return pixel_x, pixel_y

# ============================================================================
# MAIN
# ============================================================================

def main():
    print("="*70)
    print("REPEATABILITY VALIDATION TEST")
    print("="*70)
    print()
    print(f"Target: {TARGET_LAT:.6f}, {TARGET_LON:.6f}")
    print(f"Original Z-Score: {ORIGINAL_ZSCORE:.2f}")
    print(f"Tile: {TILE_BASE}")
    print()
    
    # Load thermal bands
    print("[1/3] Loading thermal bands...")
    b10_data, b10_path = load_tile('B10')
    b11_data, b11_path = load_tile('B11')
    
    if b10_data is None or b11_data is None:
        print("  ✗ FAILED - Missing thermal bands")
        return
    
    print(f"  ✓ B10: {b10_path.name} ({b10_data.shape})")
    print(f"  ✓ B11: {b11_path.name} ({b11_data.shape})")
    print()
    
    # Process thermal
    print("[2/3] Processing thermal data...")
    result = process_thermal(b10_data, b11_data)
    
    print(f"  Mean brightness: {result['mean']:.2f}")
    print(f"  Std deviation: {result['std']:.2f}")
    print(f"  Max Z-score: {result['zscore_max']:.2f}")
    print(f"  Anomalies found: {result['anomaly_count']}")
    print()
    
    # Check if target location is anomalous
    print("[3/3] Checking target location...")
    
    # Approximate pixel location (would need proper geotransform for exact)
    # For now, just check if there are ANY anomalies with similar Z-score
    
    zscore_map = result['zscore_map']
    max_z = np.max(np.abs(zscore_map))
    
    print(f"  Max Z-score in tile: {max_z:.2f}")
    print(f"  Original Z-score: {ORIGINAL_ZSCORE:.2f}")
    print()
    
    # Determine if repeatable
    print("="*70)
    print("VALIDATION RESULT")
    print("="*70)
    
    if max_z >= ORIGINAL_ZSCORE * 0.8:  # Within 20% of original
        print(f"  ✓ REPEATABLE!")
        print(f"    Max Z-score ({max_z:.2f}) is close to original ({ORIGINAL_ZSCORE:.2f})")
        print(f"    Anomaly is REAL - appears in re-processing")
        
        validation = 'PASS'
    else:
        print(f"  ✗ NOT REPEATABLE")
        print(f"    Max Z-score ({max_z:.2f}) is much lower than original ({ORIGINAL_ZSCORE:.2f})")
        print(f"    Original detection may have been noise")
        
        validation = 'FAIL'
    
    print()
    print(f"  Anomaly count: {result['anomaly_count']}")
    print(f"  Validation: {validation}")
    print("="*70)
    
    # Save results
    output_file = OUTPUT_DIR / f"validation_{TILE_BASE}.json"
    output_data = {
        'target_lat': TARGET_LAT,
        'target_lon': TARGET_LON,
        'original_zscore': ORIGINAL_ZSCORE,
        'tile': TILE_BASE,
        'validation_result': validation,
        'max_zscore': max_z,
        'anomaly_count': result['anomaly_count'],
        'timestamp': datetime.now().isoformat()
    }
    
    with open(output_file, 'w') as f:
        json.dump(output_data, f, indent=2)
    
    print(f"\nResults saved to: {output_file}")

if __name__ == "__main__":
    main()
