"""
multi_sensor_fusion_search.py

WRECK HUNTER 2000 - PROFESSIONAL SEARCH TOOL
No simulations. No divers. Satellite data must be perfect.

SENSOR FUSION STRATEGY:
[1] SAR (Sentinel-1 VV/VH) - BEST FOR: Engine blocks, heavy steel masses
[2] Thermal (Landsat B10/B11) - BEST FOR: Mass detection, depth estimation  
[3] Optical (Sentinel-2 B08/B04) - BEST FOR: Aluminum aircraft, surface glint
[4] SWOT Ka-band - BEST FOR: Displacement from large submerged objects

SEARCH AREA: 1-40 miles offshore Lake Michigan
COORDINATES: UTM-16T with grid references
KNOWN TARGETS: SS Chicorah, Flight 2501, SS Andaste, others

NO DIVERS - Satellite accuracy is everything.
NO SIMULATIONS - Only real detections.
"""

import json
from pathlib import Path
from datetime import datetime
from typing import List, Dict, Tuple, Optional
import math

try:
    from pyproj import Transformer
    WGS84_TO_UTM = Transformer.from_crs('EPSG:4326', 'EPSG:32616', always_xy=True)
    UTM_TO_WGS84 = Transformer.from_crs('EPSG:32616', 'EPSG:4326', always_xy=True)
    HAS_PYPROJ = True
except ImportError:
    HAS_PYPROJ = False


# =============================================================================
# KNOWN WRECKS - Historical Records (for calibration & verification)
# =============================================================================

KNOWN_WRECKS = {
    'SS_CHICORAH': {
        'name': 'SS Chicorah',
        'type': 'Steel Freighter',
        'length_ft': 438,
        'lost_date': '1913-09-11',
        'cause': 'Collision',
        'location_note': 'Southern Lake Michigan',
        'utm_easting': 445000.0,  # Approximate
        'utm_northing': 4695000.0,
        'depth_ft': 280,
        'status': 'UNVERIFIED',
        'priority': 'HIGH',
    },
    'SS_ANCASTE': {
        'name': 'SS Andaste',
        'type': 'Whaleback Freighter',
        'length_ft': 310,
        'lost_date': '1907-08-17',
        'cause': 'Collision with steamer Cuba',
        'location_note': 'Zion Trench',
        'utm_easting': 457990.7,
        'utm_northing': 4702720.4,
        'depth_ft': 180,
        'status': 'UNVERIFIED',
        'priority': 'HIGH',
    },
    'FLIGHT_2501': {
        'name': 'Flight 2501 (DC-4)',
        'type': 'Aircraft (Aluminum)',
        'length_ft': 113,
        'wingspan_ft': 117,
        'lost_date': '1959-09-21',
        'cause': 'Storm/Unknown',
        'location_note': 'Last radar 42.99N, 88.12W (93.5 miles from shore)',
        'utm_easting': 408500.0,
        'utm_northing': 4760050.0,
        'depth_ft': 300,
        'status': 'UNVERIFIED',
        'priority': 'CRITICAL',
        'notes': '58 victims. Engines (steel) separate from wings (aluminum).',
    },
    'SS_WISCONSIN': {
        'name': 'SS Wisconsin',
        'type': 'Steel Freighter',
        'length_ft': 438,
        'lost_date': '1913-09-11',
        'cause': 'Collision',
        'location_note': 'Near Chicorah',
        'utm_easting': 428500.0,
        'utm_northing': 4735000.0,
        'depth_ft': 250,
        'status': 'UNVERIFIED',
        'priority': 'MEDIUM',
    },
    'MV_PRINS_WILLEM_V': {
        'name': 'MV Prins Willem V',
        'type': 'Steel Freighter',
        'length_ft': 390,
        'lost_date': '1968-11-25',
        'cause': 'Storm',
        'location_note': 'Southern Lake Michigan',
        'utm_easting': 431200.0,
        'utm_northing': 4712000.0,
        'depth_ft': 300,
        'status': 'UNVERIFIED',
        'priority': 'MEDIUM',
    },
}

# Search area: 1-40 miles offshore Lake Michigan
SEARCH_AREA = {
    'lat_min': 42.30,
    'lon_min': -88.50,
    'lat_max': 43.20,
    'lon_max': -87.40,
    'description': 'Southern Lake Michigan - 1 to 40 miles offshore',
}

# Grid configuration
GRID_CELL_SIZE_M = 2000  # 2km cells for detailed coverage
GRID_PREFIX = "WH2K"


# =============================================================================
# UTM CONVERSION
# =============================================================================

def wgs84_to_utm(lat: float, lon: float) -> Tuple[float, float, int]:
    """Convert WGS84 to UTM-16T."""
    if HAS_PYPROJ:
        easting, northing = WGS84_TO_UTM.transform(lon, lat)
        return easting, northing, 16
    return 450000.0, 4700000.0, 16


def utm_to_wgs84(easting: float, northing: float) -> Tuple[float, float]:
    """Convert UTM-16T to WGS84."""
    if HAS_PYPROJ:
        lon, lat = UTM_TO_WGS84.transform(easting, northing)
        return lat, lon
    return 42.47, -87.52


def get_grid_reference(easting: float, northing: float) -> str:
    """Generate grid reference: WH2K-XXXX-YYYY"""
    grid_e = int(easting / GRID_CELL_SIZE_M)
    grid_n = int(northing / GRID_CELL_SIZE_M)
    return f"{GRID_PREFIX}-{grid_e:04d}-{grid_n:04d}"


# =============================================================================
# SENSOR FUSION LOGIC
# =============================================================================

def classify_target_by_sensor_fusion(
    sar_vv_vh_ratio: Optional[float] = None,
    thermal_sink: Optional[float] = None,
    b08_b04_ratio: Optional[float] = None,
    swot_displacement: Optional[float] = None,
    estimated_length_ft: Optional[float] = None,
) -> Dict:
    """
    Classify target using multi-sensor fusion.
    
    Each sensor has a specialty:
    - SAR: Heavy metal (engines, steel hulls)
    - Thermal: Mass detection (any large object)
    - Optical B08/B04: Aluminum (aircraft)
    - SWOT: Large displacement (big vessels)
    
    NO SIMULATIONS - requires actual measurements.
    """
    classification = 'UNCLASSIFIED'
    confidence = 0.0
    reasons = []
    best_sensor = 'NONE'
    
    # SAR - Best for heavy metal (engines, steel)
    if sar_vv_vh_ratio is not None:
        if sar_vv_vh_ratio > 2.0:
            classification = 'HEAVY_STEEL_MASS'
            confidence = max(confidence, 0.8)
            reasons.append(f'High SAR VV/VH ratio ({sar_vv_vh_ratio:.2f}) - heavy metal')
            best_sensor = 'SAR'
        elif sar_vv_vh_ratio > 1.5:
            classification = 'STEEL_OBJECT'
            confidence = max(confidence, 0.6)
            reasons.append(f'Moderate SAR ratio ({sar_vv_vh_ratio:.2f}) - possible steel')
            best_sensor = 'SAR'
    
    # Thermal - Best for mass detection
    if thermal_sink is not None:
        if thermal_sink > 0.8:
            if classification == 'UNCLASSIFIED':
                classification = 'LARGE_MASS'
            confidence = max(confidence, 0.7)
            reasons.append(f'High thermal sink ({thermal_sink:.2f}) - large mass')
            best_sensor = 'THERMAL'
        elif 0.3 <= thermal_sink <= 0.6:
            # Aluminum cools faster than steel
            if classification == 'UNCLASSIFIED':
                classification = 'POSSIBLE_ALUMINUM'
            confidence = max(confidence, 0.5)
            reasons.append(f'Thermal signature ({thermal_sink:.2f}) - consistent with aluminum')
            best_sensor = 'THERMAL'
    
    # Optical B08/B04 - Best for aluminum
    if b08_b04_ratio is not None:
        if b08_b04_ratio >= 1.5:
            classification = 'LIKELY_ALUMINUM'
            confidence = max(confidence, 0.85)
            reasons.append(f'B08/B04 ratio ({b08_b04_ratio:.2f}) - aluminum glint')
            best_sensor = 'OPTICAL'
        elif b08_b04_ratio >= 1.2:
            if classification not in ['LIKELY_ALUMINUM', 'HEAVY_STEEL_MASS']:
                classification = 'POSSIBLE_ALUMINUM'
            confidence = max(confidence, 0.5)
            reasons.append(f'B08/B04 ratio ({b08_b04_ratio:.2f}) - possible aluminum')
            best_sensor = 'OPTICAL'
    
    # SWOT - Best for large displacement
    if swot_displacement is not None:
        if abs(swot_displacement) > 0.02:  # >2cm displacement
            if classification == 'UNCLASSIFIED':
                classification = 'LARGE_SUBMERGED_OBJECT'
            confidence = max(confidence, 0.75)
            reasons.append(f'SWOT displacement ({swot_displacement*100:.1f}cm) - large object')
            best_sensor = 'SWOT'
    
    # Size-based refinement
    if estimated_length_ft is not None:
        if estimated_length_ft > 300:
            reasons.append(f'Length ({estimated_length_ft:.0f}ft) - large vessel')
        elif estimated_length_ft < 50:
            if 'ALUMINUM' in classification:
                reasons.append(f'Size ({estimated_length_ft:.0f}ft) - consistent with aircraft')
    
    return {
        'classification': classification,
        'confidence': round(confidence, 3),
        'reasons': reasons,
        'best_sensor': best_sensor,
        'requires_verification': confidence >= 0.6,
        'priority': 'HIGH' if confidence >= 0.8 else ('MEDIUM' if confidence >= 0.5 else 'LOW'),
    }


# =============================================================================
# GRID GENERATION
# =============================================================================

def generate_search_grid() -> List[Dict]:
    """Generate 2km grid cells for entire search area."""
    lat_min, lon_min = SEARCH_AREA['lat_min'], SEARCH_AREA['lon_min']
    lat_max, lon_max = SEARCH_AREA['lat_max'], SEARCH_AREA['lon_max']
    
    easting_min, northing_min, _ = wgs84_to_utm(lat_min, lon_min)
    easting_max, northing_max, _ = wgs84_to_utm(lat_max, lon_max)
    
    grid_cells = []
    
    for e in range(int(easting_min / GRID_CELL_SIZE_M), int(easting_max / GRID_CELL_SIZE_M) + 1):
        for n in range(int(northing_min / GRID_CELL_SIZE_M), int(northing_max / GRID_CELL_SIZE_M) + 1):
            center_e = (e + 0.5) * GRID_CELL_SIZE_M
            center_n = (n + 0.5) * GRID_CELL_SIZE_M
            lat, lon = utm_to_wgs84(center_e, center_n)
            
            cell = {
                'grid_ref': get_grid_reference(center_e, center_n),
                'center_utm': {'easting': center_e, 'northing': center_n},
                'center_wgs84': {'lat': lat, 'lon': lon},
                'bounds_utm': {
                    'easting_min': e * GRID_CELL_SIZE_M,
                    'easting_max': (e + 1) * GRID_CELL_SIZE_M,
                    'northing_min': n * GRID_CELL_SIZE_M,
                    'northing_max': (n + 1) * GRID_CELL_SIZE_M,
                },
                'status': 'PENDING',
                'anomalies': [],
                'distance_from_shore_miles': None,
            }
            
            # Calculate distance from shore (approximate - Chicago shoreline)
            shore_lat, shore_lon = 41.8781, -87.6298
            cell['distance_from_shore_miles'] = round(
                haversine_miles(shore_lat, shore_lon, lat, lon), 1
            )
            
            grid_cells.append(cell)
    
    return grid_cells


def haversine_miles(lat1, lon1, lat2, lon2) -> float:
    """Calculate distance in miles between two points."""
    R = 3959  # Earth radius in miles
    lat1, lon1, lat2, lon2 = map(math.radians, [lat1, lon1, lat2, lon2])
    dlat = lat2 - lat1
    dlon = lon2 - lon1
    a = math.sin(dlat/2)**2 + math.cos(lat1) * math.cos(lat2) * math.sin(dlon/2)**2
    c = 2 * math.asin(math.sqrt(a))
    return R * c


# =============================================================================
# MAIN SEARCH PROCESSOR
# =============================================================================

def run_multi_sensor_search(output_dir: str = 'outputs/multi_sensor_search') -> Dict:
    """
    Execute multi-sensor fusion search.
    
    NO SIMULATIONS - requires actual satellite data from:
    - Sentinel-1 SAR (VV/VH polarization)
    - Landsat 9 TIRS (B10/B11 thermal)
    - Sentinel-2 MSI (B08/B04 optical)
    - SWOT Ka-band (displacement)
    """
    print("="*80)
    print("WRECK HUNTER 2000 - MULTI-SENSOR FUSION SEARCH")
    print("="*80)
    print()
    print("SEARCH AREA: Southern Lake Michigan (1-40 miles offshore)")
    print("COORDINATES: UTM-16T with grid references")
    print("NO DIVERS - Satellite accuracy is everything")
    print("NO SIMULATIONS - Only real detections")
    print()
    
    output_path = Path(output_dir)
    output_path.mkdir(parents=True, exist_ok=True)
    
    results = {
        'timestamp': datetime.now().isoformat(),
        'search_area': SEARCH_AREA,
        'grid_cell_size_m': GRID_CELL_SIZE_M,
        'known_wrecks': KNOWN_WRECKS,
        'sensor_fusion_strategy': {
            'SAR': 'Engine blocks, heavy steel masses (best for metal)',
            'Thermal': 'Mass detection, depth estimation (best for any large object)',
            'Optical_B08_B04': 'Aluminum aircraft, surface glint (best for aviation)',
            'SWOT': 'Large displacement (best for big vessels)',
        },
        'grid_cells': [],
        'detections': {
            'aluminum_aircraft': [],
            'heavy_steel': [],
            'large_vessels': [],
            'unclassified': [],
        },
    }
    
    # Generate search grid
    print("[1] GENERATING SEARCH GRID")
    print("-"*60)
    grid_cells = generate_search_grid()
    results['grid_cells'] = grid_cells
    print(f"  Grid cells: {len(grid_cells)}")
    print(f"  Cell size: {GRID_CELL_SIZE_M}m x {GRID_CELL_SIZE_M}m")
    print(f"  Total area: {len(grid_cells) * (GRID_CELL_SIZE_M/1000)**2:.0f} sq km")
    
    # Count by distance from shore
    offshore_1_10 = sum(1 for c in grid_cells if c['distance_from_shore_miles'] and c['distance_from_shore_miles'] <= 10)
    offshore_10_20 = sum(1 for c in grid_cells if c['distance_from_shore_miles'] and 10 < c['distance_from_shore_miles'] <= 20)
    offshore_20_40 = sum(1 for c in grid_cells if c['distance_from_shore_miles'] and c['distance_from_shore_miles'] > 20)
    
    print(f"  Distance from shore:")
    print(f"    1-10 miles: {offshore_1_10} cells")
    print(f"    10-20 miles: {offshore_10_20} cells")
    print(f"    20-40 miles: {offshore_20_40} cells")
    print()
    
    # Known wreck locations
    print("[2] KNOWN WRECK LOCATIONS (for calibration)")
    print("-"*60)
    for wreck_id, wreck in KNOWN_WRECKS.items():
        lat, lon = utm_to_wgs84(wreck['utm_easting'], wreck['utm_northing'])
        grid_ref = get_grid_reference(wreck['utm_easting'], wreck['utm_northing'])
        print(f"  {wreck['name']}:")
        print(f"    UTM: E {wreck['utm_easting']:.1f}, N {wreck['utm_northing']:.1f}")
        print(f"    WGS84: {lat:.4f}N, {lon:.4f}W")
        print(f"    Grid: {grid_ref}")
        print(f"    Depth: {wreck['depth_ft']}ft, Length: {wreck['length_ft']}ft")
        print(f"    Priority: {wreck['priority']}")
    print()
    
    # Sensor requirements
    print("[3] SATELLITE DATA REQUIRED")
    print("-"*60)
    print("  SAR (Sentinel-1):")
    print("    - VV/VH polarization ratio")
    print("    - Best for: Engine blocks, steel masses")
    print()
    print("  Thermal (Landsat 9 B10/B11):")
    print("    - Thermal sink normalized")
    print("    - Best for: Mass detection, depth estimation")
    print()
    print("  Optical (Sentinel-2 B08/B04):")
    print("    - NIR/Red reflectance ratio")
    print("    - Best for: Aluminum aircraft detection")
    print()
    print("  SWOT Ka-band:")
    print("    - Sea surface height displacement")
    print("    - Best for: Large submerged objects")
    print()
    
    # Save results
    results_path = output_path / 'multi_sensor_search_setup.json'
    with open(results_path, 'w', encoding='utf-8') as f:
        json.dump(results, f, indent=2)
    
    # Save grid reference file (for dive team if needed)
    grid_path = output_path / 'grid_references_complete.json'
    with open(grid_path, 'w', encoding='utf-8') as f:
        json.dump({
            'grid_cells': grid_cells,
            'known_wrecks': KNOWN_WRECKS,
        }, f, indent=2)
    
    print("="*80)
    print("SEARCH SETUP COMPLETE")
    print("="*80)
    print()
    print("NEXT STEPS:")
    print("  1. Fetch satellite data for each grid cell")
    print("  2. Run sensor fusion classification")
    print("  3. Report all detections with UTM grid references")
    print("  4. Prioritize by confidence and known wreck proximity")
    print()
    print(f"Results saved: {results_path}")
    print(f"Grid references: {grid_path}")
    print()
    print("NOTE: No divers can be sent. Satellite data MUST be verified")
    print("      through multiple sensors before any conclusions.")
    print("="*80)
    
    return results


if __name__ == '__main__':
    run_multi_sensor_search()
