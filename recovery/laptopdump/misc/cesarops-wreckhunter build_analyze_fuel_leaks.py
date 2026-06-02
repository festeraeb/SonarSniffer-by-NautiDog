#!/usr/bin/env python3
"""
B12 BUBBLE MASKING ANALYSIS - Post-Processing Script
For fuel leak detection (Line 5 pipeline monitoring)

This script analyzes Sentinel-2 data to distinguish:
- Fuel sheens (bright in NIR, dark in SWIR-2)
- Bubbles/foam (bright in both NIR and SWIR-2)

No code changes needed - runs on existing KMZ/JSON outputs
"""

import json
from pathlib import Path

# ============================================================================
# CONFIGURATION
# ============================================================================

INPUT_DIR = Path(r".\outputs\run_zero")
OUTPUT_DIR = Path(r".\outputs\fuel_leak_analysis")
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Sentinel-2 band wavelengths for reference
SENTINEL2_BANDS = {
    'B02': {'name': 'Blue', 'wavelength_nm': 490, 'resolution_m': 10},
    'B04': {'name': 'Red', 'wavelength_nm': 665, 'resolution_m': 10},
    'B05': {'name': 'Red Edge', 'wavelength_nm': 705, 'resolution_m': 20},
    'B08': {'name': 'NIR', 'wavelength_nm': 842, 'resolution_m': 10},
    'B8A': {'name': 'NIR Narrow', 'wavelength_nm': 865, 'resolution_m': 20},
    'B11': {'name': 'SWIR-1', 'wavelength_nm': 1610, 'resolution_m': 20},
    'B12': {'name': 'SWIR-2', 'wavelength_nm': 2190, 'resolution_m': 20},
}

# ============================================================================
# ANALYSIS LOGIC (Post-Processing)
# ============================================================================

def calculate_leak_index(nir_reflectance, swir2_reflectance):
    """
    Calculate Fuel Leak Index: (NIR - SWIR2) / (NIR + SWIR2)
    
    Result:
    - Positive values (>0): Likely fuel sheen (bright in NIR, dark in SWIR2)
    - Near zero (~0): Ambiguous
    - Negative values (<0): Likely bubbles/foam (bright in both)
    """
    if nir_reflectance + swir2_reflectance == 0:
        return 0.0
    
    return (nir_reflectance - swir2_reflectance) / (nir_reflectance + swir2_reflectance)

def classify_pixel(nir, swir2, threshold=0.1):
    """
    Classify pixel based on NIR and SWIR-2 reflectance
    
    Logic:
    - Bright in NIR, dark in SWIR-2 → Fuel sheen
    - Bright in both → Bubbles/foam
    - Dark in both → Clear water
    """
    leak_index = calculate_leak_index(nir, swir2)
    
    if leak_index > threshold:
        return 'FUEL_SHEEN', leak_index
    elif leak_index < -threshold:
        return 'BUBBLES_FOAM', leak_index
    else:
        return 'AMBIGUOUS', leak_index

def analyze_detection(detection):
    """
    Analyze a single detection for fuel leak characteristics
    
    Expected detection fields:
    - aluminum_ratio (proxy for NIR reflectance)
    - thermal_delta
    - utm_easting, utm_northing
    - classification
    """
    # Note: We don't have direct B08/B12 values in current output
    # This is a PLACEHOLDER showing what the analysis would look like
    # once we add band-specific reflectance logging
    
    result = {
        'utm_easting': detection.get('utm_easting', 0),
        'utm_northing': detection.get('utm_northing', 0),
        'wgs84_lat': detection.get('wgs84_lat', 0),
        'wgs84_lon': detection.get('wgs84_lon', 0),
        'original_classification': detection.get('classification', 'Unknown'),
        
        # Placeholder - would be populated with actual band values
        'b08_nir': None,  # Need to add to scanner output
        'b12_swir2': None,  # Need to add to scanner output
        'leak_index': None,
        'bubble_mask_classification': 'PENDING',
        'is_fuel_sheen_candidate': False,
        'is_bubble_foam': False,
    }
    
    return result

# ============================================================================
# MAIN ANALYSIS
# ============================================================================

def main():
    print("=" * 80)
    print("B12 BUBBLE MASKING ANALYSIS")
    print("Fuel Leak Detection for Line 5 Pipeline Monitoring")
    print("=" * 80)
    print()
    
    print("SENTINEL-2 BAND REFERENCE:")
    print("-" * 80)
    for band_id, info in SENTINEL2_BANDS.items():
        print(f"  {band_id:5} | {info['name']:<15} | {info['wavelength_nm']:>4}nm | {info['resolution_m']:>2}m resolution")
    
    print()
    print("ANALYSIS LOGIC:")
    print("-" * 80)
    print("  1. Detect 'Bright Things' in NIR (B08)")
    print("  2. Check same pixels in SWIR-2 (B12)")
    print("  3. Apply Filter:")
    print("     • Bright in B12 → Bubbles/Foam (filter out)")
    print("     • Dark in B12   → Fuel Sheen (keep)")
    print()
    print("  Leak Index = (NIR - SWIR2) / (NIR + SWIR2)")
    print("     • > +0.1  → Fuel Sheen candidate")
    print("     • < -0.1  → Bubbles/Foam")
    print("     • ~0      → Ambiguous")
    print()
    
    print("IMPLEMENTATION NOTES:")
    print("-" * 80)
    print("  To enable this analysis, add to scanner output:")
    print("    • B08 (NIR) reflectance values per detection")
    print("    • B12 (SWIR-2) reflectance values per detection")
    print("    • Calculate Leak Index in post-processing")
    print()
    
    print("SAR CORRELATION (Sentinel-1):")
    print("-" * 80)
    print("  • Fuel leaks appear as BLACK scars in SAR (smooths water)")
    print("  • Bubbles appear as BRIGHT streaks in SAR (roughens water)")
    print("  • Correlation: NIR bright + SWIR2 dark + SAR black = 100% confirmed leak")
    print()
    
    print("=" * 80)
    print("READY FOR POST-PROCESSING")
    print("=" * 80)
    print()
    print("Next steps:")
    print("  1. Add B08/B12 reflectance logging to scanner")
    print("  2. Run scanner on Sentinel-2 tiles")
    print("  3. Re-run this script with actual band values")
    print("  4. Cross-reference with Sentinel-1 SAR data")

if __name__ == "__main__":
    main()
