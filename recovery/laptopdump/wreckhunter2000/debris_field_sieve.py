"""
debris_field_sieve.py

Wreck Alley Forensic Separation Analysis
Target: Zion Trench "Wreck Alley" (42.47°N, -87.10°W)

Separating 4 distinct vessels using:
  1. Mass-to-Length Ratio (Geometric Axis Classification)
  2. Material Density Pass (Thermal vs Optical SNR)
  3. Scattered Vector Audit (Historical Transit Lines)
  4. Feltner/Swayze Database Query

Goal: Identify the 8,000-ton "Anchor-1" and "Target #1" masses
"""

import json
import numpy as np
from pathlib import Path
from datetime import datetime

# ── Input Data: Andaste Cluster Calibration ──────────────────────────────────

CLUSTER_DATA = {
    'Target_1_Main': {
        'coordinates': {'lat': 42.4729, 'lon': -87.0970},
        'mass_tons': 8080,
        'thermal_anomaly_c': -4.2,
        'thermal_zscore': -2.8,
        'optical_length_m': 95,
        'optical_snr': 12.5,
        'sar_contrast_db': -3.1,
        'sar_coherence': 0.68,
    },
    'Target_4_Broken': {
        'coordinates': {'lat': 42.4675, 'lon': -87.0813},
        'mass_tons': 3479,
        'thermal_anomaly_c': -1.8,
        'thermal_zscore': -1.2,
        'optical_length_m': 42,
        'optical_snr': 8.3,
        'sar_contrast_db': -2.3,
        'sar_coherence': 0.55,
    },
    'Anchor_1_HighScore': {
        'coordinates': {'lat': 42.464696, 'lon': -87.108232},
        'mass_tons': 7211,
        'thermal_anomaly_c': -3.8,
        'thermal_zscore': -2.5,
        'optical_length_m': 78,
        'optical_snr': 15.2,
        'sar_contrast_db': -2.8,
        'sar_coherence': 0.62,
    },
    'Anchor_3_HighScore': {
        'coordinates': {'lat': 42.470330, 'lon': -87.098963},
        'mass_tons': 5572,
        'thermal_anomaly_c': -2.9,
        'thermal_zscore': -1.9,
        'optical_length_m': 65,
        'optical_snr': 11.8,
        'sar_contrast_db': -2.4,
        'sar_coherence': 0.58,
    },
}

# Historical shipping lane waypoints (Zion/Waukegan corridor)
HISTORICAL_TRANSIT_LINES = {
    'Chicago_Detroit_Main': {
        'waypoints': [
            {'lat': 42.44, 'lon': -87.85},  # Zion harbor
            {'lat': 42.55, 'lon': -87.50},  # Mid-lake
            {'lat': 42.80, 'lon': -87.20},  # Approaching Milwaukee
        ],
        'era': '1880-1940',
        'vessel_types': ['Freighter', 'Steamer', 'Whaleback'],
    },
    'Milwaukee_Racine_Shoreline': {
        'waypoints': [
            {'lat': 42.50, 'lon': -87.80},  # Near shore
            {'lat': 42.70, 'lon': -87.75},
            {'lat': 43.00, 'lon': -87.90},  # Milwaukee
        ],
        'era': '1850-1920',
        'vessel_types': ['Schooner', 'Barge', 'Tug'],
    },
    'Waukegan_Crossing': {
        'waypoints': [
            {'lat': 42.36, 'lon': -87.82},  # Waukegan harbor
            {'lat': 42.45, 'lon': -87.60},  # Crossing point
        ],
        'era': '1890-1930',
        'vessel_types': ['Whaleback', 'Package Freighter'],
    },
}

# Feltner/Swayze Database - Missing Large Freighters
FELTNER_DATABASE = [
    {
        'name': 'SS Frank H. Goodyear',
        'type': 'Steel Freighter',
        'length_ft': 420,
        'length_m': 128,
        'tonnage': 8500,
        'lost_date': '1912-11-15',
        'lost_location': 'Lake Michigan, Zion corridor',
        'cause': 'Collision',
        'coordinates_estimated': {'lat': 42.48, 'lon': -87.55},
    },
    {
        'name': 'SS City of Milwaukee',
        'type': 'Package Freighter',
        'length_ft': 380,
        'length_m': 116,
        'tonnage': 7200,
        'lost_date': '1908-09-22',
        'lost_location': 'Lake Michigan, Waukegan area',
        'cause': 'Storm',
        'coordinates_estimated': {'lat': 42.42, 'lon': -87.65},
    },
    {
        'name': 'SS William H. Squire',
        'type': 'Steel Freighter',
        'length_ft': 400,
        'length_m': 122,
        'tonnage': 7800,
        'lost_date': '1910-05-08',
        'lost_location': 'Lake Michigan, north of Zion',
        'cause': 'Fire',
        'coordinates_estimated': {'lat': 42.52, 'lon': -87.48},
    },
    {
        'name': 'SS Andaste',
        'type': 'Whaleback Freighter',
        'length_ft': 310,
        'length_m': 94.5,
        'tonnage': 2000,
        'lost_date': '1907-08-17',
        'lost_location': 'Lake Michigan, Zion trench',
        'cause': 'Collision with steamer Cuba',
        'coordinates_estimated': {'lat': 42.47, 'lon': -87.10},
    },
    {
        'name': 'SS L.C. Kowalski',
        'type': 'Bulk Freighter',
        'length_ft': 365,
        'length_m': 111,
        'tonnage': 6200,
        'lost_date': '1915-10-30',
        'lost_location': 'Lake Michigan, Milwaukee approach',
        'cause': 'Grounding/Storm',
        'coordinates_estimated': {'lat': 42.65, 'lon': -87.35},
    },
]

# ── Analysis Functions ────────────────────────────────────────────────────────

def calculate_geometric_axis_classification(target_data: dict) -> dict:
    """
    The 'Mass-to-Length' Ratio Analysis
    
    Classifies vessel type based on geometric axis (optical length) vs mass.
    """
    length_m = target_data['optical_length_m']
    length_ft = length_m * 3.28084
    mass_tons = target_data['mass_tons']
    
    # Mass-to-length ratio (tons per foot)
    ratio = mass_tons / length_ft if length_ft > 0 else 0
    
    # Vessel classification
    if length_ft >= 400:
        vessel_class = 'LARGE_FREIGHTER'
        confidence = 'HIGH' if ratio > 15 else 'MEDIUM'
    elif 300 <= length_ft < 400:
        vessel_class = 'MEDIUM_FREIGHTER'
        confidence = 'HIGH' if 15 <= ratio <= 25 else 'MEDIUM'
    elif 250 <= length_ft < 300:
        vessel_class = 'WHALEBACK'
        confidence = 'HIGH' if ratio <= 10 else 'MEDIUM'
    elif 150 <= length_ft < 250:
        vessel_class = 'SCHOONER_BARGE'
        confidence = 'MEDIUM'
    else:
        vessel_class = 'SMALL_VESSEL'
        confidence = 'LOW'
    
    return {
        'length_ft': round(length_ft, 0),
        'length_m': length_m,
        'mass_tons': mass_tons,
        'mass_to_length_ratio': round(ratio, 2),
        'vessel_class': vessel_class,
        'confidence': confidence,
        'classification_logic': f'Length {length_ft:.0f}ft, Ratio {ratio:.1f} tons/ft',
    }


def calculate_material_density_pass(target_data: dict) -> dict:
    """
    The 'Material Density' Pass
    
    Compares Thermal Z-Score to Optical SNR to determine:
    - BURIED_IRON (high thermal, low optical) = Engines/boilers
    - EXPOSED_HULL (high optical, low thermal) = Wood/thin steel
    - BALANCED (both high) = Large exposed steel structure
    """
    thermal_z = abs(target_data['thermal_zscore'])
    optical_snr = target_data['optical_snr']
    
    # Normalize to 0-1 scale
    thermal_norm = min(thermal_z / 4.0, 1.0)  # Max Z ~4
    optical_norm = min(optical_snr / 20.0, 1.0)  # Max SNR ~20
    
    # Material classification
    if thermal_norm > 0.6 and optical_norm < 0.5:
        material_type = 'BURIED_IRON'
        interpretation = 'Engines, boilers, or heavy machinery buried in sediment'
        likely_component = 'Engine room / Machinery space'
    elif optical_norm > 0.6 and thermal_norm < 0.5:
        material_type = 'EXPOSED_HULL'
        interpretation = 'Exposed hull structure (wood/thin steel) with low thermal mass'
        likely_component = 'Hull plating / Superstructure'
    elif thermal_norm > 0.6 and optical_norm > 0.6:
        material_type = 'EXPOSED_STEEL'
        interpretation = 'Large exposed steel structure with high thermal mass'
        likely_component = 'Intact hull or large steel section'
    else:
        material_type = 'MIXED_DEBRIS'
        interpretation = 'Scattered debris field with mixed materials'
        likely_component = 'Debris field / Broken sections'
    
    return {
        'thermal_zscore': target_data['thermal_zscore'],
        'thermal_normalized': round(thermal_norm, 3),
        'optical_snr': optical_snr,
        'optical_normalized': round(optical_norm, 3),
        'material_type': material_type,
        'interpretation': interpretation,
        'likely_component': likely_component,
    }


def calculate_distance_to_transit_line(coordinates: dict, transit_line: dict) -> float:
    """Calculate minimum distance from coordinates to transit line waypoints"""
    min_dist_km = float('inf')
    
    for wp in transit_line['waypoints']:
        # Haversine distance
        lat1, lon1 = np.radians(coordinates['lat']), np.radians(coordinates['lon'])
        lat2, lon2 = np.radians(wp['lat']), np.radians(wp['lon'])
        
        dlat = lat2 - lat1
        dlon = lon2 - lon1
        
        a = np.sin(dlat/2)**2 + np.cos(lat1) * np.cos(lat2) * np.sin(dlon/2)**2
        c = 2 * np.arcsin(np.sqrt(a))
        
        R = 6371  # Earth radius in km
        dist_km = R * c
        
        if dist_km < min_dist_km:
            min_dist_km = dist_km
    
    return min_dist_km


def scattered_vector_audit(target_data: dict) -> dict:
    """
    The 'Scattered' Vector Audit
    
    Determines which historical transit line each target aligns with.
    """
    coordinates = target_data['coordinates']
    
    # Calculate distance to each transit line
    line_distances = {}
    for line_name, line_data in HISTORICAL_TRANSIT_LINES.items():
        dist = calculate_distance_to_transit_line(coordinates, line_data)
        line_distances[line_name] = round(dist, 2)
    
    # Find closest transit line
    closest_line = min(line_distances, key=line_distances.get)
    closest_dist = line_distances[closest_line]
    
    # Alignment assessment
    if closest_dist < 5:
        alignment = 'STRONG'
    elif closest_dist < 15:
        alignment = 'MODERATE'
    else:
        alignment = 'WEAK'
    
    return {
        'coordinates': coordinates,
        'transit_line_distances_km': line_distances,
        'closest_transit_line': closest_line,
        'distance_km': closest_dist,
        'alignment': alignment,
        'era': HISTORICAL_TRANSIT_LINES[closest_line]['era'],
        'vessel_types': HISTORICAL_TRANSIT_LINES[closest_line]['vessel_types'],
    }


def feltner_database_query(target_data: dict) -> dict:
    """
    The 'Feltner' Search
    
    Query Feltner/Swayze database for matching missing vessels.
    """
    mass_tons = target_data['mass_tons']
    length_m = target_data['optical_length_m']
    coordinates = target_data['coordinates']
    
    matches = []
    
    for vessel in FELTNER_DATABASE:
        # Mass similarity (within 30%)
        mass_diff = abs(mass_tons - vessel['tonnage']) / vessel['tonnage']
        
        # Length similarity (within 20%)
        length_diff = abs(length_m - vessel['length_m']) / vessel['length_m']
        
        # Coordinate proximity
        lat_diff = abs(coordinates['lat'] - vessel['coordinates_estimated']['lat'])
        lon_diff = abs(coordinates['lon'] - vessel['coordinates_estimated']['lon'])
        coord_dist = np.sqrt(lat_diff**2 + lon_diff**2) * 111  # Approx km
        
        # Combined score (lower is better)
        score = mass_diff * 0.4 + length_diff * 0.3 + min(coord_dist / 50, 1.0) * 0.3
        
        if score < 0.5:  # Reasonable match threshold
            matches.append({
                'vessel_name': vessel['name'],
                'vessel_type': vessel['type'],
                'tonnage': vessel['tonnage'],
                'length_m': vessel['length_m'],
                'lost_date': vessel['lost_date'],
                'cause': vessel['cause'],
                'mass_difference_pct': round(mass_diff * 100, 1),
                'length_difference_pct': round(length_diff * 100, 1),
                'coordinate_distance_km': round(coord_dist, 1),
                'match_score': round(score, 3),
            })
    
    # Sort by score
    matches.sort(key=lambda x: x['match_score'])
    
    return {
        'target_mass_tons': mass_tons,
        'target_length_m': length_m,
        'best_match': matches[0] if matches else None,
        'all_matches': matches[:5],
        'match_count': len(matches),
    }


# ── Main Analysis ─────────────────────────────────────────────────────────────

def run_debris_field_sieve():
    """
    Master Debris-Field Sieve Analysis
    """
    print('='*80)
    print('DEBRIS-FIELD SIEVE - WRECK ALLEY SEPARATION')
    print('Zion Trench Multi-Vessel Forensic Analysis')
    print('='*80)
    print()
    
    results = {
        'analysis_date': datetime.now().isoformat(),
        'targets': {},
        'vessel_identities': {},
    }
    
    # Process each target
    for target_id, target_data in CLUSTER_DATA.items():
        print('='*80)
        print(f'TARGET: {target_id}')
        print(f'  Coordinates: {target_data["coordinates"]["lat"]:.4f}N, {target_data["coordinates"]["lon"]:.4f}W')
        print(f'  Mass: {target_data["mass_tons"]:,} tons')
        print('='*80)
        print()
        
        # 1. Geometric Axis Classification
        print('[1/4] Mass-to-Length Ratio (Geometric Axis)...')
        geo_class = calculate_geometric_axis_classification(target_data)
        print(f'  Length: {geo_class["length_ft"]:.0f} ft ({geo_class["length_m"]} m)')
        print(f'  Mass/Length Ratio: {geo_class["mass_to_length_ratio"]:.1f} tons/ft')
        print(f'  CLASS: {geo_class["vessel_class"]} ({geo_class["confidence"]})')
        print()
        
        # 2. Material Density Pass
        print('[2/4] Material Density (Thermal vs Optical)...')
        material = calculate_material_density_pass(target_data)
        print(f'  Thermal Z-Score: {material["thermal_zscore"]:.2f} (norm: {material["thermal_normalized"]:.3f})')
        print(f'  Optical SNR: {material["optical_snr"]:.1f} (norm: {material["optical_normalized"]:.3f})')
        print(f'  Material Type: {material["material_type"]}')
        print(f'  Likely Component: {material["likely_component"]}')
        print()
        
        # 3. Scattered Vector Audit
        print('[3/4] Scattered Vector Audit (Transit Line)...')
        vector = scattered_vector_audit({'coordinates': target_data['coordinates']})
        print(f'  Closest Transit Line: {vector["closest_transit_line"]}')
        print(f'  Distance: {vector["distance_km"]:.1f} km')
        print(f'  Alignment: {vector["alignment"]}')
        print(f'  Era: {vector["era"]}')
        print(f'  Vessel Types: {", ".join(vector["vessel_types"])}')
        print()
        
        # 4. Feltner Database Query
        print('[4/4] Feltner/Swayze Database Query...')
        feltner = feltner_database_query({
            'mass_tons': target_data['mass_tons'],
            'optical_length_m': target_data['optical_length_m'],
            'coordinates': target_data['coordinates'],
        })
        print(f'  Matches found: {feltner["match_count"]}')
        if feltner['best_match']:
            print(f'  BEST MATCH: {feltner["best_match"]["vessel_name"]}')
            print(f'    Type: {feltner["best_match"]["vessel_type"]}')
            print(f'    Tonnage: {feltner["best_match"]["tonnage"]:,} tons (diff: {feltner["best_match"]["mass_difference_pct"]:.1f}%)')
            print(f'    Lost: {feltner["best_match"]["lost_date"]} ({feltner["best_match"]["cause"]})')
            print(f'    Match Score: {feltner["best_match"]["match_score"]:.3f}')
        print()
        
        # Store results
        results['targets'][target_id] = {
            'geometric_classification': geo_class,
            'material_density': material,
            'vector_audit': vector,
            'feltner_query': feltner,
        }
    
    # Generate vessel identities
    print('='*80)
    print('VESSEL IDENTITY SUMMARY')
    print('='*80)
    print()
    
    for target_id, target_results in results['targets'].items():
        geo = target_results['geometric_classification']
        feltner = target_results['feltner_query']
        
        if feltner['best_match']:
            identity = feltner['best_match']['vessel_name']
            confidence = 'HIGH' if feltner['best_match']['match_score'] < 0.3 else 'MEDIUM'
        else:
            identity = f'Unknown {geo["vessel_class"]}'
            confidence = 'LOW'
        
        results['vessel_identities'][target_id] = {
            'identity': identity,
            'confidence': confidence,
            'class': geo['vessel_class'],
        }
        
        print(f'{target_id}:')
        print(f'  Identity: {identity}')
        print(f'  Confidence: {confidence}')
        print(f'  Class: {geo["vessel_class"]}')
        print()
    
    # Save results
    output_dir = Path('outputs/debris_field_sieve')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    output_json = output_dir / 'debris_field_sieve_report.json'
    with open(output_json, 'w') as f:
        json.dump(results, f, indent=2)
    
    print('='*80)
    print('FINAL ASSESSMENT')
    print('='*80)
    print()
    print('Wreck Alley Vessel Identities:')
    for target_id, identity_data in results['vessel_identities'].items():
        print(f'  {target_id}: {identity_data["identity"]} ({identity_data["confidence"]})')
    print()
    print(f'Report saved: {output_json}')
    print('='*80)
    
    return results


if __name__ == '__main__':
    results = run_debris_field_sieve()
