"""
offshore_border_search.py

Search for signatures 10+ miles offshore, closer to the Wisconsin-Michigan state line border.

Target area:
- Distance from shore: 10-20 miles (16-32 km)
- Latitude: 43.00-43.50°N (approaching state line)
- Longitude: -87.70 to -87.50°W (farther offshore)
"""

import json
import numpy as np
from pathlib import Path
from datetime import datetime

# ── Configuration ─────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'offshore_border_search'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Load previous Fall 2025 results
FALL_2025_FILE = REPO / 'outputs' / 'fall_2025_ice_free' / 'fall_2025_new_signatures.json'

# Extended offshore corridor
OFFSHORE_CORRIDOR = {
    'name': 'Offshore Border Corridor (10-20 miles)',
    'bbox': [-87.75, 43.00, -87.50, 43.50],  # lon_min, lat_min, lon_max, lat_max
    'distance_from_shore_miles': (10, 20),
    'distance_from_shore_km': (16, 32),
}

# Wisconsin-Michigan state line is at ~43.25°N in Lake Michigan
STATE_LINE_LAT = 43.25

# ── Analysis Functions ────────────────────────────────────────────────────────

def distance_from_shore_miles(lat: float, lon: float) -> float:
    """
    Estimate distance from Wisconsin shoreline in miles.
    
    Shoreline approx: -87.87 to -87.90 longitude
    """
    shore_lon = -87.88
    km_per_deg_lon = 111 * np.cos(np.radians(lat))
    distance_km = abs(lon - shore_lon) * km_per_deg_lon
    distance_miles = distance_km / 1.609
    return distance_miles


def distance_to_state_line(lat: float, lon: float) -> float:
    """
    Distance to Wisconsin-Michigan state line (43.25°N).
    """
    lat_diff = abs(lat - STATE_LINE_LAT)
    km_per_deg_lat = 111
    distance_km = lat_diff * km_per_deg_lat
    return distance_km


def filter_offshore_border_signatures(signatures: list[dict]) -> list[dict]:
    """Filter for 10+ miles offshore, closer to state line"""
    
    filtered = []
    
    for sig in signatures:
        # Check distance from shore (10-20 miles)
        dist_shore = distance_from_shore_miles(sig['lat'], sig['lon'])
        if not (OFFSHORE_CORRIDOR['distance_from_shore_miles'][0] <= dist_shore <= OFFSHORE_CORRIDOR['distance_from_shore_miles'][1]):
            continue
        
        # Check distance to state line (within 30km of 43.25°N)
        dist_state = distance_to_state_line(sig['lat'], sig['lon'])
        if dist_state > 30:  # More than 30km from state line
            continue
        
        # Apply minimum quality thresholds
        if sig['signature_score'] < 0.30:
            continue
        if sig['edge_density'] < 0.08:
            continue
        
        # Add computed distances
        sig['distance_from_shore_miles'] = round(dist_shore, 2)
        sig['distance_to_state_line_km'] = round(dist_state, 2)
        
        filtered.append(sig)
    
    return filtered


def generate_new_offshore_grid() -> list[dict]:
    """Generate new search grid for offshore border area"""
    
    grid_points = []
    
    lat_step = 0.01  # ~1km spacing
    lon_step = 0.014  # ~1km at this latitude
    
    lat = OFFSHORE_CORRIDOR['bbox'][1]
    while lat <= OFFSHORE_CORRIDOR['bbox'][3]:
        lon = OFFSHORE_CORRIDOR['bbox'][0]
        while lon <= OFFSHORE_CORRIDOR['bbox'][2]:
            # Check if in offshore range
            dist = distance_from_shore_miles(lat, lon)
            if OFFSHORE_CORRIDOR['distance_from_shore_miles'][0] <= dist <= OFFSHORE_CORRIDOR['distance_from_shore_miles'][1]:
                grid_points.append({
                    'lat': round(lat, 5),
                    'lon': round(lon, 5),
                    'id': f'OFFSHORE-{len(grid_points)+1:04d}',
                    'distance_miles': round(dist, 2),
                })
            lon += lon_step
        lat += lat_step
    
    return grid_points


# ── Main Analysis ─────────────────────────────────────────────────────────────

def main():
    print('='*80)
    print('OFFSHORE BORDER SEARCH')
    print('10-20 Miles Offshore, Near Wisconsin-Michigan State Line')
    print('='*80)
    print()
    
    # Generate new offshore grid
    print('Generating offshore search grid...')
    grid_points = generate_new_offshore_grid()
    print(f'  Grid points: {len(grid_points)}')
    print(f'  Area: {OFFSHORE_CORRIDOR["name"]}')
    print(f'  Distance from shore: {OFFSHORE_CORRIDOR["distance_from_shore_miles"]} miles')
    print(f'  Distance to state line: Within 30km of 43.25°N')
    print()
    
    # Load Fall 2025 signatures
    if not FALL_2025_FILE.exists():
        print(f'[!] Fall 2025 data not found: {FALL_2025_FILE}')
        all_signatures = []
    else:
        with open(FALL_2025_FILE) as f:
            data = json.load(f)
        all_signatures = data.get('new_signatures', [])
        print(f'Loaded {len(all_signatures)} Fall 2025 signatures')
    
    # Filter for offshore border area
    print()
    print('Filtering for offshore border signatures...')
    print(f'  Distance from shore: 10-20 miles')
    print(f'  Distance to state line: <30km')
    print(f'  Min score: 0.30')
    print()
    
    offshore_signatures = filter_offshore_border_signatures(all_signatures)
    print(f'Found {len(offshore_signatures)} signatures in offshore border area')
    print()
    
    # Cluster nearby signatures
    if offshore_signatures:
        # Simple clustering
        clusters = []
        used = set()
        
        for i, sig in enumerate(offshore_signatures):
            if i in used:
                continue
            
            cluster_members = [sig]
            used.add(i)
            
            for j, other in enumerate(offshore_signatures):
                if j in used:
                    continue
                
                dist = np.sqrt((sig['lat'] - other['lat'])**2 + (sig['lon'] - other['lon'])**2)
                if dist < 0.02:  # Within ~2km
                    cluster_members.append(other)
                    used.add(j)
            
            if cluster_members:
                center_lat = np.mean([m['lat'] for m in cluster_members])
                center_lon = np.mean([m['lon'] for m in cluster_members])
                
                cluster = {
                    'id': f'OFFSHORE-BORDER-{len(clusters)+1:03d}',
                    'lat': round(center_lat, 5),
                    'lon': round(center_lon, 5),
                    'members': len(cluster_members),
                    'max_score': max([m['signature_score'] for m in cluster_members]),
                    'avg_ndwi': np.mean([m['ndwi_anomaly'] for m in cluster_members]),
                    'avg_turbidity': np.mean([m['turbidity_ratio'] for m in cluster_members]),
                    'avg_edge': np.mean([m['edge_density'] for m in cluster_members]),
                    'distance_from_shore_miles': cluster_members[0]['distance_from_shore_miles'],
                    'distance_to_state_line_km': cluster_members[0]['distance_to_state_line_km'],
                }
                clusters.append(cluster)
        
        # Sort by score
        clusters.sort(key=lambda x: -x['max_score'])
        
        print('='*80)
        print('OFFSHORE BORDER CLUSTERS (10-20 miles offshore)')
        print('='*80)
        print()
        
        if clusters:
            for i, cluster in enumerate(clusters[:15], 1):
                print(f'{i:2}. {cluster["id"]} | {cluster["lat"]:.4f}N, {cluster["lon"]:.4f}W')
                print(f'    Distance from shore: {cluster["distance_from_shore_miles"]:.1f} miles')
                print(f'    Distance to state line: {cluster["distance_to_state_line_km"]:.1f} km')
                print(f'    Members: {cluster["members"]} | Max Score: {cluster["max_score"]:.3f}')
                print(f'    NDWI: {cluster["avg_ndwi"]:+.3f} | Turbidity: {cluster["avg_turbidity"]:.2f}× | Edge: {cluster["avg_edge"]:.1%}')
                print()
            
            print('='*80)
            print('SUMMARY')
            print('='*80)
            print()
            print(f'Total offshore border clusters: {len(clusters)}')
            print()
            
            if clusters:
                print('TOP 5 OFFSHORE TARGETS:')
                for cluster in clusters[:5]:
                    print(f'  {cluster["id"]}: {cluster["lat"]:.4f}N, {cluster["lon"]:.4f}W')
                    print(f'    {cluster["distance_from_shore_miles"]:.1f} miles offshore, '
                          f'{cluster["distance_to_state_line_km"]:.1f} km from state line')
                    print()
        else:
            print('No clusters found in offshore border area.')
            print('Signatures in this area may be below detection threshold.')
    
    else:
        print('No Fall 2025 signatures found in offshore border area.')
        print('This area may not have been covered in the original scan.')
        clusters = []
    
    # Save results
    results = {
        'analysis_date': datetime.now().isoformat(),
        'corridor': OFFSHORE_CORRIDOR,
        'grid_points': len(grid_points),
        'fall_2025_signatures_loaded': len(all_signatures),
        'offshore_signatures_found': len(offshore_signatures),
        'clusters': clusters if offshore_signatures else [],
    }
    
    output_json = OUTPUT_DIR / 'offshore_border_results.json'
    with open(output_json, 'w') as f:
        json.dump(results, f, indent=2)
    
    print(f'Results saved: {output_json}')
    print('='*80)
    
    return clusters


if __name__ == '__main__':
    main()
