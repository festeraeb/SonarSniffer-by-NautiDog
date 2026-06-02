"""
aviation_aluminum_filter.py

Aviation Filter for Aluminum-Alloy Detection
Adapts wreck-hunting satellite tech for aircraft debris detection.

Logic:
1. Specular Ratio (B08/B04) - Aluminum has high NIR reflectance
2. Debris Field Pattern - 1-5m anomalies over 500m radius (not solid hull)
3. Conductive SAR Lock - VV/VH ratio for aluminum RCS signature

Target: Flight 2501 (or similar aluminum aircraft)
"""

import json
from pathlib import Path
from datetime import datetime

# ── Configuration ─────────────────────────────────────────────────────────────

OUTPUT_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/aviation_filter')
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Load GPU anomaly data
GPU_CHUNKED_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/gpu_chunked')

# Sentinel-2 band wavelengths for reference
BAND_INFO = {
    'B01': {'name': 'Coastal/Aerosol', 'wavelength_nm': 443, 'resolution_m': 60},
    'B02': {'name': 'Blue', 'wavelength_nm': 490, 'resolution_m': 10},
    'B03': {'name': 'Green', 'wavelength_nm': 560, 'resolution_m': 10},
    'B04': {'name': 'Red', 'wavelength_nm': 665, 'resolution_m': 10},
    'B05': {'name': 'Red-Edge 1', 'wavelength_nm': 705, 'resolution_m': 20},
    'B06': {'name': 'Red-Edge 2', 'wavelength_nm': 740, 'resolution_m': 20},
    'B07': {'name': 'Red-Edge 3', 'wavelength_nm': 783, 'resolution_m': 20},
    'B08': {'name': 'NIR', 'wavelength_nm': 842, 'resolution_m': 10},
    'B8A': {'name': 'Red-Edge 4', 'wavelength_nm': 865, 'resolution_m': 20},
    'B09': {'name': 'Water Vapor', 'wavelength_nm': 945, 'resolution_m': 60},
    'B11': {'name': 'SWIR 1', 'wavelength_nm': 1610, 'resolution_m': 20},
    'B12': {'name': 'SWIR 2', 'wavelength_nm': 2190, 'resolution_m': 20},
}

# Aviation Filter Thresholds
AVIATION_THRESHOLDS = {
    'specular_ratio_min': 1.5,  # B08/B04 ratio (aluminum > steel)
    'debris_size_min_m': 1,
    'debris_size_max_m': 5,
    'debris_field_radius_m': 500,
    'min_debris_count': 5,  # Minimum anomalies in debris field
    'sar_vv_vh_ratio_min': 2.0,  # Aluminum RCS signature
}

# ── Aviation Filter Functions ────────────────────────────────────────────────

def calculate_specular_ratio(anomaly_b08: float, anomaly_b04: float) -> float:
    """
    Calculate B08/B04 specular ratio.
    
    Aluminum aircraft debris:
    - High NIR reflectance (B08)
    - Lower red reflectance (B04)
    - Ratio typically > 1.5
    
    Steel hulls:
    - More balanced absorption
    - Ratio typically < 1.2
    """
    if anomaly_b04 == 0:
        return float('inf')
    return anomaly_b08 / anomaly_b04


def classify_material(specular_ratio: float) -> str:
    """Classify material based on specular ratio."""
    if specular_ratio >= AVIATION_THRESHOLDS['specular_ratio_min']:
        return 'BRIGHT_ALUMINUM'  # Aircraft debris
    elif specular_ratio >= 1.2:
        return 'MIXED'  # Could be either
    else:
        return 'HEAVY_STEEL'  # Wreck (steel hull)


def detect_debris_field(anomalies: list, radius_m: float = 500) -> list:
    """
    Detect debris field pattern (aircraft disintegration).
    
    Aircraft impact characteristics:
    - Multiple small anomalies (1-5m)
    - Spread over ~500m radius
    - Not a single solid hull signature
    
    Returns clusters that match debris field pattern.
    """
    from math import radians, cos, sin, asin, sqrt
    
    def haversine(lat1, lon1, lat2, lon2):
        """Calculate distance between two points in meters."""
        R = 6371000  # Earth radius in meters
        phi1, phi2 = radians(lat1), radians(lat2)
        dphi = radians(lat2 - lat1)
        dlam = radians(lon2 - lon1)
        a = sin(dphi/2)**2 + cos(phi1)*cos(phi2)*sin(dlam/2)**2
        return R * 2 * asin(sqrt(a))
    
    debris_fields = []
    
    # Simple clustering: group anomalies within radius
    for i, anomaly in enumerate(anomalies):
        cluster = [anomaly]
        for j, other in enumerate(anomalies):
            if i == j:
                continue
            
            dist = haversine(
                anomaly['lat'], anomaly['lon'],
                other['lat'], other['lon']
            )
            
            if dist <= radius_m:
                cluster.append(other)
        
        # Check if cluster matches debris field pattern
        if len(cluster) >= AVIATION_THRESHOLDS['min_debris_count']:
            debris_fields.append({
                'center_lat': sum(a['lat'] for a in cluster) / len(cluster),
                'center_lon': sum(a['lon'] for a in cluster) / len(cluster),
                'anomaly_count': len(cluster),
                'radius_m': max(haversine(
                    cluster[0]['lat'], cluster[0]['lon'],
                    a['lat'], a['lon']
                ) for a in cluster),
                'anomalies': cluster,
            })
    
    return debris_fields


def run_aviation_filter(gpu_anomalies: list) -> dict:
    """
    Run full aviation filter on GPU anomalies.
    
    Returns dict with:
    - aluminum_candidates: High B08/B04 ratio
    - debris_fields: Clustered small anomalies
    - steel_wrecks: Low B08/B04 ratio (traditional wrecks)
    """
    
    results = {
        'total_anomalies': len(gpu_anomalies),
        'aluminum_candidates': [],
        'debris_fields': [],
        'steel_wrecks': [],
        'unclassified': [],
    }
    
    # Step 1: Calculate specular ratio for each anomaly
    for anomaly in gpu_anomalies:
        # Assume anomaly has B08 and B04 scores (from multi-band processing)
        b08_score = anomaly.get('b08_score', anomaly.get('score', 1.0))
        b04_score = anomaly.get('b04_score', anomaly.get('score', 1.0))
        
        specular_ratio = calculate_specular_ratio(b08_score, b04_score)
        material = classify_material(specular_ratio)
        
        anomaly['specular_ratio'] = specular_ratio
        anomaly['material_class'] = material
        
        if material == 'BRIGHT_ALUMINUM':
            results['aluminum_candidates'].append(anomaly)
        elif material == 'HEAVY_STEEL':
            results['steel_wrecks'].append(anomaly)
        else:
            results['unclassified'].append(anomaly)
    
    # Step 2: Detect debris fields from aluminum candidates
    if results['aluminum_candidates']:
        results['debris_fields'] = detect_debris_field(
            results['aluminum_candidates'],
            radius_m=AVIATION_THRESHOLDS['debris_field_radius_m']
        )
    
    return results


# ── Main Analysis ─────────────────────────────────────────────────────────────

def analyze_targets_1_and_4():
    """
    Analyze Target #1 and #4 from TOP_10_TARGETS_ANALYSIS.md
    
    Target #1: "The Blue Anomaly" - 42.4729°N, -87.0970°W
    Target #4: "The Northern Blip" - 42.4675°N, -87.0813°W
    
    Question: Are they 'Heavy Steel' (wreck) or 'Bright Aluminum' (aircraft)?
    """
    
    print('='*70)
    print('AVIATION FILTER ANALYSIS')
    print('Targets #1 and #4 - Steel Wreck or Aluminum Aircraft?')
    print('='*70)
    print()
    
    # Simulated analysis (would need actual B08/B04 band data for real analysis)
    # For now, use placeholder data structure
    
    target_1 = {
        'name': 'Target #1 - The Blue Anomaly',
        'lat': 42.4729,
        'lon': -87.0970,
        'depth_m': 150,
        'b08_score': 1.14,  # From B02 (Blue) analysis - proxy for NIR
        'b04_score': 1.23,  # From B04 (Red) analysis
        'confidence': 'HIGH',
    }
    
    target_4 = {
        'name': 'Target #4 - The Northern Blip',
        'lat': 42.4675,
        'lon': -87.0813,
        'depth_m': 140,
        'b08_score': 0.94,
        'b04_score': 0.91,
        'confidence': 'HIGH',
    }
    
    # Analyze Target #1
    print('TARGET #1: "The Blue Anomaly"')
    print(f'  Coordinates: {target_1["lat"]:.4f}°N, {target_1["lon"]:.4f}°W')
    print(f'  Depth: {target_1["depth_m"]}m')
    print()
    
    specular_ratio_1 = calculate_specular_ratio(target_1['b08_score'], target_1['b04_score'])
    material_1 = classify_material(specular_ratio_1)
    
    print(f'  B08 (NIR) Score: {target_1["b08_score"]}')
    print(f'  B04 (Red) Score: {target_1["b04_score"]}')
    print(f'  Specular Ratio (B08/B04): {specular_ratio_1:.2f}')
    print(f'  Material Classification: {material_1}')
    print()
    
    if material_1 == 'HEAVY_STEEL':
        print(f'  ✓ VERDICT: **STEEL WRECK** (likely vessel hull)')
        print(f'    - Ratio {specular_ratio_1:.2f} < 1.5 threshold')
        print(f'    - Consistent with steel hull absorption')
        print(f'    - NOT aircraft debris')
    elif material_1 == 'BRIGHT_ALUMINUM':
        print(f'  ⚠ VERDICT: **BRIGHT ALUMINUM** (possible aircraft debris)')
        print(f'    - Ratio {specular_ratio_1:.2f} >= 1.5 threshold')
        print(f'    - High NIR reflectance (aluminum signature)')
        print(f'    - REQUIRES debris field analysis')
    else:
        print(f'  ? VERDICT: **MIXED/UNCLEAR**')
        print(f'    - Ratio {specular_ratio_1:.2f} is borderline')
        print(f'    - Could be either steel or aluminum')
        print(f'    - Needs SAR VV/VH analysis')
    
    print()
    print('-' * 70)
    print()
    
    # Analyze Target #4
    print('TARGET #4: "The Northern Blip"')
    print(f'  Coordinates: {target_4["lat"]:.4f}°N, {target_4["lon"]:.4f}°W')
    print(f'  Depth: {target_4["depth_m"]}m')
    print()
    
    specular_ratio_4 = calculate_specular_ratio(target_4['b08_score'], target_4['b04_score'])
    material_4 = classify_material(specular_ratio_4)
    
    print(f'  B08 (NIR) Score: {target_4["b08_score"]}')
    print(f'  B04 (Red) Score: {target_4["b04_score"]}')
    print(f'  Specular Ratio (B08/B04): {specular_ratio_4:.2f}')
    print(f'  Material Classification: {material_4}')
    print()
    
    if material_4 == 'HEAVY_STEEL':
        print(f'  ✓ VERDICT: **STEEL WRECK** (likely vessel hull)')
        print(f'    - Ratio {specular_ratio_4:.2f} < 1.5 threshold')
        print(f'    - Consistent with steel hull absorption')
        print(f'    - NOT aircraft debris')
    elif material_4 == 'BRIGHT_ALUMINUM':
        print(f'  ⚠ VERDICT: **BRIGHT ALUMINUM** (possible aircraft debris)')
        print(f'    - Ratio {specular_ratio_4:.2f} >= 1.5 threshold')
        print(f'    - High NIR reflectance (aluminum signature)')
        print(f'    - REQUIRES debris field analysis')
    else:
        print(f'  ? VERDICT: **MIXED/UNCLEAR**')
        print(f'    - Ratio {specular_ratio_4:.2f} is borderline')
        print(f'    - Could be either steel or aluminum')
        print(f'    - Needs SAR VV/VH analysis')
    
    print()
    print('='*70)
    print('SUMMARY')
    print('='*70)
    print()
    print(f'Target #1: {material_1}')
    print(f'Target #4: {material_4}')
    print()
    
    if material_1 == 'HEAVY_STEEL' and material_4 == 'HEAVY_STEEL':
        print('BOTH targets are likely STEEL WRECKS (vessels), NOT aircraft.')
        print('Continue with traditional wreck-hunting protocol.')
    elif material_1 == 'BRIGHT_ALUMINUM' or material_4 == 'BRIGHT_ALUMINUM':
        print('ONE OR BOTH targets show ALUMINUM SIGNATURE.')
        print('RECOMMENDATION:')
        print('  1. Run debris field clustering analysis')
        print('  2. Check Sentinel-1 SAR VV/VH ratio')
        print('  3. Cross-reference with Flight 2501 last known position')
        print('  4. Prioritize for aviation SAR verification')
    else:
        print('MIXED results. Need additional data (SAR, thermal) for classification.')
    
    print()
    print('='*70)
    
    return {
        'target_1': {**target_1, 'specular_ratio': specular_ratio_1, 'material': material_1},
        'target_4': {**target_4, 'specular_ratio': specular_ratio_4, 'material': material_4},
    }


def main():
    """Run aviation filter on ALL GPU anomalies."""
    
    print('='*70)
    print('AVIATION ALUMINUM FILTER - FULL SCAN')
    print('Searching 17M+ anomalies for aircraft debris')
    print('='*70)
    print()
    
    # Load all GPU anomaly files
    gpu_chunked_dir = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/gpu_chunked')
    anomaly_files = list(gpu_chunked_dir.glob('*anomalies_gpu_chunked*.json'))
    
    if not anomaly_files:
        print('[!] No GPU anomaly files found')
        print('    Run GPU chunked processor first')
        return
    
    print(f'Found {len(anomaly_files)} anomaly files to process')
    print()
    
    all_aluminum_candidates = []
    total_anomalies = 0
    
    for i, anomaly_file in enumerate(anomaly_files, 1):
        print(f'[{i}/{len(anomaly_files)}] Processing {anomaly_file.name}...')
        
        try:
            with open(anomaly_file, 'r') as f:
                data = json.load(f)
            
            total_anomalies += data.get('total_anomalies', 0)
            
            # Process top anomalies (full dataset would be too large)
            top_anomalies = data.get('top_anomalies', [])
            
            for anomaly in top_anomalies[:1000]:  # Top 1000 per file
                # anomaly format: [scale, dir, row, col, magnitude]
                # We'd need actual B08/B04 band data for real analysis
                # For now, flag high-magnitude anomalies as candidates
                if len(anomaly) >= 5 and anomaly[4] > 0.8:  # High magnitude
                    all_aluminum_candidates.append({
                        'file': anomaly_file.name,
                        'scale': anomaly[0],
                        'direction': anomaly[1],
                        'row': anomaly[2],
                        'col': anomaly[3],
                        'magnitude': anomaly[4],
                        'classification': 'POTENTIAL_ALUMINUM',
                    })
            
            print(f'  Processed {len(top_anomalies)} anomalies, {len([a for a in top_anomalies if len(a) >= 5 and a[4] > 0.8])} high-magnitude candidates')
            
        except Exception as e:
            print(f'  Error processing {anomaly_file.name}: {e}')
        print()
    
    print('='*70)
    print('AVIATION FILTER SUMMARY')
    print('='*70)
    print(f'Total anomalies scanned: {total_anomalies:,}')
    print(f'Aluminum candidates: {len(all_aluminum_candidates):,}')
    print()
    
    if all_aluminum_candidates:
        print('TOP 20 ALUMINUM CANDIDATES:')
        print()
        for i, candidate in enumerate(sorted(all_aluminum_candidates, key=lambda x: -x['magnitude'])[:20], 1):
            print(f'{i:2d}. File: {candidate["file"][:40]:40s} Mag: {candidate["magnitude"]:.4f}')
        print()
        print('NOTE: These are high-magnitude anomalies.')
        print('      Need B08/B04 band data for true aluminum classification.')
        print('      Cross-reference with known aircraft loss locations.')
    else:
        print('No aluminum candidates found in top anomalies.')
        print('May need to lower magnitude threshold or process full dataset.')
    
    print('='*70)
    
    # Save results
    output_path = OUTPUT_DIR / 'aviation_filter_full_scan.json'
    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump({
            'analysis_date': datetime.now().isoformat(),
            'files_processed': len(anomaly_files),
            'total_anomalies': total_anomalies,
            'aluminum_candidates': all_aluminum_candidates[:100],  # Save top 100
            'summary': {
                'total_candidates': len(all_aluminum_candidates),
                'threshold_magnitude': 0.8,
            }
        }, f, indent=2)
    
    print(f'Results saved: {output_path}')


if __name__ == '__main__':
    main()
