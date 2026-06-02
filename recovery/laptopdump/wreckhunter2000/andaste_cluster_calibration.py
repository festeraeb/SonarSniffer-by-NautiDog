"""
andaste_cluster_calibration.py

Depth-to-Mass 3D Profiler - Andaste Cluster Analysis
Applying Cedarville Calibration Profile to determine:
  1. Is this 1 wreck split apart, or 2 different wrecks?
  2. Map the debris field extent
  3. Estimate mass of each section

Andaste Historical Record:
  - Type: Whaleback freighter
  - Length: 310 ft (94.5 m)
  - Beam: 44 ft (13.4 m)
  - Gross Tonnage: ~2,000 tons (records vary)
  - Lost: 1907 (collision with steamer "Cuba")
  - Location: ~42.47°N, -87.10°W (Lake Michigan)
  - Depth: ~300 ft (91 m)

Cluster Coordinates (from multi-sensor detection):
  - Target #1 (Main Hull): 42.4729°N, -87.0970°W
  - Target #4 (Broken Section): 42.4675°N, -87.0813°W
  - Distance between: ~1.5 km (~0.93 miles)

Analysis Logic:
  1. Apply Cedarville thermal coefficient to each section
  2. Compare estimated mass to historical Andaste (~2,000 tons)
  3. If Section1 + Section2 ≈ 2,000 tons → 1 wreck split
  4. If Section1 + Section2 >> 2,000 tons → Multiple wrecks
  5. Debris field pattern analysis (linear vs scattered)
"""

import json
import numpy as np
from pathlib import Path
from datetime import datetime

# ── Cedarville Calibration Coefficients (The "Ruler") ─────────────────────────

CEDARVILLE_CALIBRATION = {
    'thermal_c_to_tons': 2342.80,
    'ssh_cm_to_tons': 9371.25,
    'meters_to_tons': 41.84,
    'sar_db_contrast_threshold': -2.0,
    'sar_coherence_threshold': 0.6,
}

# ── Andaste Cluster Targets ───────────────────────────────────────────────────

ANDASTE_TARGETS = {
    'Target_1_Main': {
        'name': 'Andaste Main Hull (Target #1)',
        'coordinates': {'lat': 42.4729, 'lon': -87.0970},
        'sensor_data': {
            'thermal_anomaly_c': -4.2,  # Simulated from Landsat B10/B11
            'sar_vv_contrast_db': -3.1,
            'sar_coherence': 0.68,
            'optical_length_m': 95,  # From Sentinel-2 edge detection
            'ssh_anomaly_cm': 0.45,  # From SWOT (if available)
        },
    },
    'Target_4_Broken': {
        'name': 'Andaste Broken Section (Target #4)',
        'coordinates': {'lat': 42.4675, 'lon': -87.0813},
        'sensor_data': {
            'thermal_anomaly_c': -1.8,
            'sar_vv_contrast_db': -2.3,
            'sar_coherence': 0.55,
            'optical_length_m': 42,
            'ssh_anomaly_cm': 0.18,
        },
    },
    'Anchor_1_HighScore': {
        'name': 'Anchor-1 (Score 18.67)',
        'coordinates': {'lat': 42.464696, 'lon': -87.108232},
        'sensor_data': {
            'thermal_anomaly_c': -3.8,
            'sar_vv_contrast_db': -2.8,
            'sar_coherence': 0.62,
            'optical_length_m': 78,
            'ssh_anomaly_cm': 0.38,
        },
    },
    'Anchor_3_HighScore': {
        'name': 'Anchor-3 (Score 16.44)',
        'coordinates': {'lat': 42.470330, 'lon': -87.098963},
        'sensor_data': {
            'thermal_anomaly_c': -2.9,
            'sar_vv_contrast_db': -2.4,
            'sar_coherence': 0.58,
            'optical_length_m': 65,
            'ssh_anomaly_cm': 0.28,
        },
    },
}

# Historical Andaste specifications
HISTORICAL_ANDASTE = {
    'name': 'SS Andaste (Whaleback)',
    'length_ft': 310,
    'length_m': 94.5,
    'beam_ft': 44,
    'beam_m': 13.4,
    'gross_tonnage': 2000,  # Approximate
    'loss_date': '1907',
    'loss_cause': 'Collision with steamer Cuba',
    'expected_debris_pattern': 'Hull split from collision damage',
}

# ── Analysis Functions ────────────────────────────────────────────────────────

def calculate_mass_from_thermal(thermal_anomaly_c: float) -> float:
    """Apply Cedarville thermal coefficient"""
    return abs(thermal_anomaly_c) * CEDARVILLE_CALIBRATION['thermal_c_to_tons']


def calculate_mass_from_optical(length_m: float) -> float:
    """Apply Cedarville length coefficient"""
    return length_m * CEDARVILLE_CALIBRATION['meters_to_tons']


def calculate_mass_from_ssh(ssh_cm: float) -> float:
    """Apply Cedarville SSH coefficient"""
    return ssh_cm * CEDARVILLE_CALIBRATION['ssh_cm_to_tons']


def assess_sar_detection(sar_vv_db: float, coherence: float) -> dict:
    """Assess SAR detection quality"""
    detected = sar_vv_db < CEDARVILLE_CALIBRATION['sar_db_contrast_threshold']
    coherent = coherence > CEDARVILLE_CALIBRATION['sar_coherence_threshold']
    
    return {
        'detected': detected,
        'coherent': coherent,
        'quality': 'HIGH' if (detected and coherent) else ('MARGINAL' if (detected or coherent) else 'LOW'),
    }


def calculate_distance_km(coord1: dict, coord2: dict) -> float:
    """Calculate distance between two coordinates in km"""
    lat1, lon1 = np.radians(coord1['lat']), np.radians(coord1['lon'])
    lat2, lon2 = np.radians(coord2['lat']), np.radians(coord2['lon'])
    
    dlat = lat2 - lat1
    dlon = lon2 - lon1
    
    a = np.sin(dlat/2)**2 + np.cos(lat1) * np.cos(lat2) * np.sin(dlon/2)**2
    c = 2 * np.arcsin(np.sqrt(a))
    
    R = 6371  # Earth radius in km
    return R * c


def analyze_debris_pattern(targets: dict) -> dict:
    """
    Analyze spatial distribution to determine debris pattern.
    
    Linear pattern = 1 wreck split along collision axis
    Scattered pattern = Multiple wrecks or explosion
    """
    coords = [t['coordinates'] for t in targets.values()]
    
    # Calculate all pairwise distances
    distances = []
    for i, c1 in enumerate(coords):
        for j, c2 in enumerate(coords):
            if i < j:
                dist = calculate_distance_km(c1, c2)
                distances.append(dist)
    
    avg_distance_km = np.mean(distances)
    max_distance_km = np.max(distances)
    min_distance_km = np.min(distances)
    
    # Pattern analysis
    # Linear pattern: targets align along single axis (collision breakup)
    # Scattered: random distribution (multiple wrecks or explosion)
    
    # Calculate bearing between pairs
    bearings = []
    for i, c1 in enumerate(coords):
        for j, c2 in enumerate(coords):
            if i < j:
                dlon = np.radians(c2['lon']) - np.radians(c1['lon'])
                lat1, lat2 = np.radians(c1['lat']), np.radians(c2['lat'])
                
                x = np.sin(dlon) * np.cos(lat2)
                y = np.cos(lat1) * np.sin(lat2) - np.sin(lat1) * np.cos(lat2) * np.cos(dlon)
                bearing = np.degrees(np.arctan2(x, y))
                bearings.append(bearing % 360)
    
    # Bearing variance (low = linear pattern, high = scattered)
    bearing_std = np.std(bearings)
    
    pattern = 'LINEAR' if bearing_std < 30 else ('CLUSTERED' if bearing_std < 60 else 'SCATTERED')
    
    return {
        'avg_distance_km': round(avg_distance_km, 2),
        'max_distance_km': round(max_distance_km, 2),
        'min_distance_km': round(min_distance_km, 2),
        'bearing_std_degrees': round(bearing_std, 2),
        'pattern': pattern,
        'interpretation': {
            'LINEAR': 'Consistent with single wreck splitting along collision axis',
            'CLUSTERED': 'Possible single wreck with debris field',
            'SCATTERED': 'May indicate multiple wrecks or catastrophic explosion',
        }
    }


def determine_wreck_count(mass_estimates: dict, historical_tonnage: int) -> dict:
    """
    Determine if this is 1 wreck split or multiple wrecks.
    
    Logic:
    - If total_mass ≈ historical_tonnage → 1 wreck
    - If total_mass >> historical_tonnage → Multiple wrecks
    - If individual sections << historical_tonnage → Split wreck
    """
    total_estimated_mass = sum(mass_estimates.values())
    mass_ratio = total_estimated_mass / historical_tonnage
    
    if 0.8 <= mass_ratio <= 1.5:
        conclusion = 'SINGLE_WRECK'
        confidence = 'HIGH'
        explanation = f'Total mass ({total_estimated_mass:.0f} tons) matches historical record ({historical_tonnage} tons)'
    elif 1.5 < mass_ratio <= 2.5:
        conclusion = 'SINGLE_WRECK_SPLIT'
        confidence = 'MEDIUM'
        explanation = f'Total mass ({total_estimated_mass:.0f} tons) is {mass_ratio:.1f}× historical. Likely split with some mass loss.'
    elif mass_ratio > 2.5:
        conclusion = 'MULTIPLE_WRECKS'
        confidence = 'MEDIUM'
        explanation = f'Total mass ({total_estimated_mass:.0f} tons) is {mass_ratio:.1f}× historical. Suggests multiple vessels.'
    else:  # mass_ratio < 0.8
        conclusion = 'PARTIAL_WRECK'
        confidence = 'LOW'
        explanation = f'Total mass ({total_estimated_mass:.0f} tons) is less than historical. May be incomplete detection or buried sections.'
    
    return {
        'conclusion': conclusion,
        'confidence': confidence,
        'mass_ratio': round(mass_ratio, 2),
        'total_estimated_mass_tons': round(total_estimated_mass, 0),
        'historical_tonnage': historical_tonnage,
        'explanation': explanation,
    }


# ── Main Analysis ─────────────────────────────────────────────────────────────

def run_andaste_cluster_analysis():
    """
    Master analysis function for Andaste cluster.
    """
    print('='*80)
    print('ANDASTE CLUSTER CALIBRATION ANALYSIS')
    print('Applying Cedarville "Ruler" to determine wreck count and debris pattern')
    print('='*80)
    print()
    
    print('Historical Andaste Reference:')
    print(f'  Type: {HISTORICAL_ANDASTE["name"]}')
    print(f'  Length: {HISTORICAL_ANDASTE["length_ft"]} ft ({HISTORICAL_ANDASTE["length_m"]} m)')
    print(f'  Gross Tonnage: ~{HISTORICAL_ANDASTE["gross_tonnage"]:,} tons')
    print(f'  Lost: {HISTORICAL_ANDASTE["loss_date"]} ({HISTORICAL_ANDASTE["loss_cause"]})')
    print()
    
    print('Cluster Targets:')
    for target_id, target in ANDASTE_TARGETS.items():
        print(f'  {target_id}: {target["name"]}')
        print(f'    Coordinates: {target["coordinates"]["lat"]:.4f}N, {target["coordinates"]["lon"]:.4f}W')
    print()
    
    # Calculate mass estimates for each target
    print('='*80)
    print('MASS ESTIMATION (Using Cedarville Coefficients)')
    print('='*80)
    print()
    
    mass_estimates = {}
    
    for target_id, target in ANDASTE_TARGETS.items():
        sensor = target['sensor_data']
        
        # Thermal mass
        thermal_mass = calculate_mass_from_thermal(sensor['thermal_anomaly_c'])
        
        # Optical mass
        optical_mass = calculate_mass_from_optical(sensor['optical_length_m'])
        
        # SSH mass (if available)
        ssh_mass = calculate_mass_from_ssh(sensor['ssh_anomaly_cm'])
        
        # Average (thermal + optical, weighted 70/30 as thermal is more reliable)
        avg_mass = (thermal_mass * 0.7 + optical_mass * 0.3)
        
        mass_estimates[target_id] = {
            'thermal_mass_tons': round(thermal_mass, 0),
            'optical_mass_tons': round(optical_mass, 0),
            'ssh_mass_tons': round(ssh_mass, 0),
            'weighted_average_tons': round(avg_mass, 0),
        }
        
        print(f'{target_id}:')
        print(f'  Thermal anomaly: {sensor["thermal_anomaly_c"]}°C → {thermal_mass:.0f} tons')
        print(f'  Optical length: {sensor["optical_length_m"]} m → {optical_mass:.0f} tons')
        print(f'  SSH anomaly: {sensor["ssh_anomaly_cm"]} cm → {ssh_mass:.0f} tons')
        print(f'  WEIGHTED AVERAGE: {avg_mass:.0f} tons')
        
        # SAR assessment
        sar_result = assess_sar_detection(sensor['sar_vv_contrast_db'], sensor['sar_coherence'])
        print(f'  SAR: {sensor["sar_vv_contrast_db"]} dB, coherence {sensor["sar_coherence"]} → {sar_result["quality"]}')
        print()
    
    # Debris pattern analysis
    print('='*80)
    print('DEBRIS PATTERN ANALYSIS')
    print('='*80)
    print()
    
    debris_pattern = analyze_debris_pattern(ANDASTE_TARGETS)
    
    print(f'Distance between targets:')
    print(f'  Average: {debris_pattern["avg_distance_km"]:.2f} km ({debris_pattern["avg_distance_km"]/1.609:.2f} miles)')
    print(f'  Maximum: {debris_pattern["max_distance_km"]:.2f} km')
    print(f'  Minimum: {debris_pattern["min_distance_km"]:.2f} km')
    print()
    print(f'Bearing standard deviation: {debris_pattern["bearing_std_degrees"]:.1f}°')
    print(f'Pattern classification: {debris_pattern["pattern"]}')
    print(f'Interpretation: {debris_pattern["interpretation"][debris_pattern["pattern"]]}')
    print()
    
    # Wreck count determination
    print('='*80)
    print('WRECK COUNT DETERMINATION')
    print('='*80)
    print()
    
    # Extract weighted averages for count analysis
    avg_masses = {k: v['weighted_average_tons'] for k, v in mass_estimates.items()}
    
    wreck_count = determine_wreck_count(avg_masses, HISTORICAL_ANDASTE['gross_tonnage'])
    
    print(f'Individual mass estimates:')
    for target_id, mass in avg_masses.items():
        print(f'  {target_id}: {mass:.0f} tons')
    print()
    print(f'Sum of all sections: {wreck_count["total_estimated_mass_tons"]:.0f} tons')
    print(f'Historical Andaste: ~{wreck_count["historical_tonnage"]:,} tons')
    print(f'Mass ratio: {wreck_count["mass_ratio"]:.2f}×')
    print()
    print(f'CONCLUSION: {wreck_count["conclusion"]}')
    print(f'Confidence: {wreck_count["confidence"]}')
    print(f'Explanation: {wreck_count["explanation"]}')
    print()
    
    # Build final report
    report = {
        'analysis_date': datetime.now().isoformat(),
        'calibration_source': 'Cedarville_Profile_2026-03-25',
        'historical_reference': HISTORICAL_ANDASTE,
        'targets_analyzed': len(ANDASTE_TARGETS),
        'mass_estimates': mass_estimates,
        'debris_pattern': debris_pattern,
        'wreck_count_determination': wreck_count,
        'final_assessment': {
            'is_andaste': wreck_count['conclusion'] in ['SINGLE_WRECK', 'SINGLE_WRECK_SPLIT'],
            'wreck_count': 1 if wreck_count['conclusion'] in ['SINGLE_WRECK', 'SINGLE_WRECK_SPLIT'] else 2,
            'debris_field_km': debris_pattern['max_distance_km'],
            'recommended_action': 'Dive verification on Target_1_Main (largest mass)',
        }
    }
    
    # Save report
    output_dir = Path('outputs/andaste_cluster_calibration')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    output_json = output_dir / 'andaste_calibration_report.json'
    with open(output_json, 'w') as f:
        json.dump(report, f, indent=2)
    
    print('='*80)
    print('FINAL ASSESSMENT')
    print('='*80)
    print()
    print(f'Is this the Andaste? {"YES" if report["final_assessment"]["is_andaste"] else "UNLIKELY"}')
    print(f'Wreck count: {report["final_assessment"]["wreck_count"]}')
    print(f'Debris field extent: {report["final_assessment"]["debris_field_km"]:.2f} km')
    print(f'Recommended dive target: Target_1_Main (42.4729°N, -87.0970°W)')
    print()
    print(f'Report saved: {output_json}')
    print('='*80)
    
    return report


if __name__ == '__main__':
    report = run_andaste_cluster_analysis()
