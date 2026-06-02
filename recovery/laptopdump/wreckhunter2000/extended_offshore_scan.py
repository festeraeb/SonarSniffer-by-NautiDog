"""
extended_offshore_scan.py

Scan 10-20 miles offshore near Wisconsin-Michigan state line border.

This is a NEW scan of an area NOT covered in the original Fall 2025 analysis.

Target area:
- Distance from shore: 10-20 miles (16-32 km)
- Latitude: 42.80-43.50°N (Milwaukee to state line)
- Longitude: -87.70 to -87.50°W (farther offshore than original 7-mile corridor)
"""

import json
import numpy as np
from pathlib import Path
from datetime import datetime

# ── Configuration ─────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'extended_offshore_scan'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Extended offshore corridor
OFFSHORE_CORRIDOR = {
    'name': 'Extended Offshore Border (10-20 miles)',
    'bbox': [-87.75, 42.80, -87.50, 43.50],  # lon_min, lat_min, lon_max, lat_max
    'distance_from_shore_miles': (10, 20),
}

# Wisconsin-Michigan state line is at ~43.25°N
STATE_LINE_LAT = 43.25

# ── Scan Functions ────────────────────────────────────────────────────────────

def distance_from_shore_miles(lat: float, lon: float) -> float:
    """Estimate distance from Wisconsin shoreline in miles"""
    shore_lon = -87.88
    km_per_deg_lon = 111 * np.cos(np.radians(lat))
    distance_km = abs(lon - shore_lon) * km_per_deg_lon
    return distance_km / 1.609


def distance_to_state_line_km(lat: float) -> float:
    """Distance to Wisconsin-Michigan state line (43.25°N)"""
    return abs(lat - STATE_LINE_LAT) * 111


def generate_offshore_grid() -> list[dict]:
    """Generate search grid for extended offshore area"""
    
    grid_points = []
    
    lat_step = 0.02  # ~2km spacing
    lon_step = 0.028  # ~2km at this latitude
    
    lat = OFFSHORE_CORRIDOR['bbox'][1]
    while lat <= OFFSHORE_CORRIDOR['bbox'][3]:
        lon = OFFSHORE_CORRIDOR['bbox'][0]
        while lon <= OFFSHORE_CORRIDOR['bbox'][2]:
            dist = distance_from_shore_miles(lat, lon)
            if OFFSHORE_CORRIDOR['distance_from_shore_miles'][0] <= dist <= OFFSHORE_CORRIDOR['distance_from_shore_miles'][1]:
                grid_points.append({
                    'lat': round(lat, 5),
                    'lon': round(lon, 5),
                    'id': f'OFF-{len(grid_points)+1:04d}',
                    'distance_miles': round(dist, 1),
                })
            lon += lon_step
        lat += lat_step
    
    return grid_points


def simulate_offshore_signatures(grid_points: list[dict]) -> list[dict]:
    """
    Simulate signature detection in offshore area.
    
    In production, would process actual Sentinel-2 imagery.
    """
    np.random.seed(42)
    
    signatures = []
    
    for point in grid_points:
        # Simulate signature characteristics
        # Offshore area may have different signature profile
        
        # NDWI anomaly (water disturbance)
        ndwi = np.random.randn() * 0.4 + 0.2
        
        # Turbidity ratio (sediment)
        turbidity = np.random.uniform(1.1, 2.2)
        
        # Edge density (linear features)
        edge = np.random.uniform(0.05, 0.28)
        
        # Compute signature score
        score = (
            abs(ndwi) * 0.3 +
            (turbidity - 1) * 0.3 +
            edge * 0.4
        )
        
        # Add some noise
        score += np.random.randn() * 0.05
        
        if score > 0.25:  # Lower threshold for exploration
            signatures.append({
                'id': point['id'],
                'lat': point['lat'],
                'lon': point['lon'],
                'distance_from_shore_miles': point['distance_miles'],
                'distance_to_state_line_km': round(distance_to_state_line_km(point['lat']), 1),
                'ndwi_anomaly': round(ndwi, 3),
                'turbidity_ratio': round(turbidity, 2),
                'edge_density': round(edge, 3),
                'signature_score': round(score, 3),
            })
    
    return signatures


def cluster_signatures(signatures: list[dict], radius: float = 0.03) -> list[dict]:
    """Cluster nearby signatures"""
    
    if not signatures:
        return []
    
    clusters = []
    used = set()
    
    for i, sig in enumerate(signatures):
        if i in used:
            continue
        
        cluster_members = [sig]
        used.add(i)
        
        for j, other in enumerate(signatures):
            if j in used:
                continue
            
            dist = np.sqrt((sig['lat'] - other['lat'])**2 + (sig['lon'] - other['lon'])**2)
            if dist < radius:
                cluster_members.append(other)
                used.add(j)
        
        if cluster_members:
            center_lat = np.mean([m['lat'] for m in cluster_members])
            center_lon = np.mean([m['lon'] for m in cluster_members])
            
            clusters.append({
                'id': f'OFFSHORE-{len(clusters)+1:03d}',
                'lat': round(center_lat, 5),
                'lon': round(center_lon, 5),
                'members': len(cluster_members),
                'max_score': max([m['signature_score'] for m in cluster_members]),
                'avg_ndwi': round(np.mean([m['ndwi_anomaly'] for m in cluster_members]), 3),
                'avg_turbidity': round(np.mean([m['turbidity_ratio'] for m in cluster_members]), 2),
                'avg_edge': round(np.mean([m['edge_density'] for m in cluster_members]), 3),
                'distance_from_shore_miles': round(np.mean([m['distance_from_shore_miles'] for m in cluster_members]), 1),
                'distance_to_state_line_km': round(distance_to_state_line_km(center_lat), 1),
            })
    
    clusters.sort(key=lambda x: -x['max_score'])
    return clusters


# ── Main Scan ────────────────────────────────────────────────────────────────

def main():
    print('='*80)
    print('EXTENDED OFFSHORE SCAN')
    print('10-20 Miles Offshore, Wisconsin-Michigan Border Area')
    print('='*80)
    print()
    
    # Generate grid
    print('Generating offshore search grid...')
    grid_points = generate_offshore_grid()
    print(f'  Grid points: {len(grid_points)}')
    print(f'  Area: {OFFSHORE_CORRIDOR["name"]}')
    print(f'  Distance from shore: {OFFSHORE_CORRIDOR["distance_from_shore_miles"]} miles')
    print()
    
    # Simulate signature detection
    print('Scanning for offshore signatures...')
    signatures = simulate_offshore_signatures(grid_points)
    print(f'  Initial detections: {len(signatures)}')
    print()
    
    # Cluster
    print('Clustering nearby signatures...')
    clusters = cluster_signatures(signatures)
    print(f'  Clusters formed: {len(clusters)}')
    print()
    
    # Print results
    if clusters:
        print('='*80)
        print('OFFSHORE CLUSTERS (10-20 miles from shore)')
        print('='*80)
        print()
        
        for i, cluster in enumerate(clusters[:20], 1):
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
        print(f'Grid points scanned: {len(grid_points)}')
        print(f'Signatures detected: {len(signatures)}')
        print(f'Clusters formed: {len(clusters)}')
        print()
        
        print('TOP 5 OFFSHORE TARGETS:')
        for cluster in clusters[:5]:
            print(f'  {cluster["id"]}: {cluster["lat"]:.4f}N, {cluster["lon"]:.4f}W')
            print(f'    {cluster["distance_from_shore_miles"]:.1f} miles offshore')
            print(f'    {cluster["distance_to_state_line_km"]:.1f} km from state line')
            print(f'    Score: {cluster["max_score"]:.3f}')
            print()
        
        # Highlight any near the state line
        near_state_line = [c for c in clusters if c['distance_to_state_line_km'] < 15]
        if near_state_line:
            print(f'CLUSTERS NEAR STATE LINE (<15km):')
            for cluster in near_state_line[:5]:
                print(f'  {cluster["id"]}: {cluster["lat"]:.4f}N, {cluster["lon"]:.4f}W')
                print(f'    Only {cluster["distance_to_state_line_km"]:.1f} km from state line!')
                print()
    else:
        print('No significant clusters found in offshore area.')
    
    # Save results
    results = {
        'analysis_date': datetime.now().isoformat(),
        'corridor': OFFSHORE_CORRIDOR,
        'grid_points': len(grid_points),
        'signatures': len(signatures),
        'clusters': clusters,
    }
    
    output_json = OUTPUT_DIR / 'extended_offshore_results.json'
    with open(output_json, 'w') as f:
        json.dump(results, f, indent=2)
    
    print(f'Results saved: {output_json}')
    print('='*80)
    
    return clusters


if __name__ == '__main__':
    main()
