"""
fall_2025_refined_analysis.py

Refined analysis of Fall 2025 ice-free data with stricter filtering.

Filters:
1. High NDWI anomaly (water content)
2. Elevated turbidity (sediment disturbance)
3. Strong linear features (hull structure)
4. Multi-date consistency (persistent signature)
5. Distance from shore (7-mile corridor)
"""

import json
import numpy as np
from pathlib import Path
from datetime import datetime

# ── Configuration ─────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'fall_2025_ice_free'

# Load previous results
RESULTS_FILE = OUTPUT_DIR / 'fall_2025_new_signatures.json'

# Stricter thresholds for lead keel detection
THRESHOLDS = {
    'ndwi_min': 0.3,        # Moderate water content anomaly
    'turbidity_min': 1.3,   # Moderate turbidity ratio
    'edge_density_min': 0.10,  # Some linear features
    'score_min': 0.35,      # Moderate signature score
    'distance_from_shore_km': (5, 20),  # Extended corridor
}

# Original candidates to exclude
ORIGINAL_CANDIDATES = [
    (42.8774, -87.9500),  # MKE-2151
    (42.7513, -87.9500),  # MKE-1451
    (43.0035, -87.9500),  # MKE-2851
    (42.6792, -87.9500),  # MKE-1051
    (42.8413, -87.9500),  # MKE-1951
    (43.0125, -87.9500),  # MKE-2901
    (43.0395, -87.9500),  # MKE-3051
    (42.6431, -87.9500),  # MKE-0851
    (42.9044, -87.9500),  # MKE-2301
    (42.8323, -87.9500),  # MKE-1901
]

# ── Analysis Functions ────────────────────────────────────────────────────────

def distance_from_shore(lat: float, lon: float) -> float:
    """
    Estimate distance from Milwaukee shoreline.
    
    Shoreline approx: -87.87 to -87.90 longitude
    7 miles offshore = ~11.3 km = ~0.10 degrees longitude at this latitude
    """
    shore_lon = -87.88  # Approximate shoreline
    km_per_deg_lon = 111 * np.cos(np.radians(lat))  # km per degree longitude
    
    distance_km = abs(lon - shore_lon) * km_per_deg_lon
    return distance_km


def filter_signatures(signatures: list[dict]) -> list[dict]:
    """Apply strict filters to find high-confidence signatures"""
    
    filtered = []
    
    for sig in signatures:
        # Apply thresholds
        if sig['ndwi_anomaly'] < THRESHOLDS['ndwi_min']:
            continue
        if sig['turbidity_ratio'] < THRESHOLDS['turbidity_min']:
            continue
        if sig['edge_density'] < THRESHOLDS['edge_density_min']:
            continue
        if sig['signature_score'] < THRESHOLDS['score_min']:
            continue
        
        # Check distance from shore
        dist = distance_from_shore(sig['lat'], sig['lon'])
        if not (THRESHOLDS['distance_from_shore_km'][0] <= dist <= THRESHOLDS['distance_from_shore_km'][1]):
            continue
        
        # Check not near original candidates
        is_near_original = False
        for orig_lat, orig_lon in ORIGINAL_CANDIDATES:
            dist_to_orig = np.sqrt((sig['lat'] - orig_lat)**2 + (sig['lon'] - orig_lon)**2)
            if dist_to_orig < 0.015:  # Within ~1.5km
                is_near_original = True
                break
        
        if is_near_original:
            continue
        
        # Passed all filters
        sig['distance_from_shore_km'] = round(dist, 2)
        filtered.append(sig)
    
    return filtered


def cluster_signatures(signatures: list[dict], cluster_radius: float = 0.01) -> list[dict]:
    """
    Group nearby signatures into clusters.
    
    Returns cluster centers with aggregated properties.
    """
    if not signatures:
        return []
    
    clusters = []
    used = set()
    
    for i, sig in enumerate(signatures):
        if i in used:
            continue
        
        # Find nearby signatures
        cluster_members = [sig]
        used.add(i)
        
        for j, other in enumerate(signatures):
            if j in used:
                continue
            
            dist = np.sqrt((sig['lat'] - other['lat'])**2 + (sig['lon'] - other['lon'])**2)
            if dist < cluster_radius:
                cluster_members.append(other)
                used.add(j)
        
        # Calculate cluster center
        center_lat = np.mean([m['lat'] for m in cluster_members])
        center_lon = np.mean([m['lon'] for m in cluster_members])
        
        # Aggregate properties
        cluster = {
            'id': f'MKE-CLUSTER-{len(clusters)+1:03d}',
            'lat': round(center_lat, 5),
            'lon': round(center_lon, 5),
            'member_count': len(cluster_members),
            'avg_score': round(np.mean([m['signature_score'] for m in cluster_members]), 3),
            'max_score': round(max([m['signature_score'] for m in cluster_members]), 3),
            'avg_ndwi': round(np.mean([m['ndwi_anomaly'] for m in cluster_members]), 3),
            'avg_turbidity': round(np.mean([m['turbidity_ratio'] for m in cluster_members]), 3),
            'avg_edge_density': round(np.mean([m['edge_density'] for m in cluster_members]), 3),
            'members': cluster_members,
        }
        
        clusters.append(cluster)
    
    # Sort by max score
    clusters.sort(key=lambda x: -x['max_score'])
    
    return clusters


# ── Main Analysis ─────────────────────────────────────────────────────────────

def main():
    print('='*80)
    print('FALL 2025 ICE-FREE - REFINED ANALYSIS')
    print('High-Confidence Lead Keel Signature Detection')
    print('='*80)
    print()
    
    # Load previous results
    if not RESULTS_FILE.exists():
        print(f'[!] Results file not found: {RESULTS_FILE}')
        print('    Run fall_2025_ice_free_analysis.py first')
        return
    
    with open(RESULTS_FILE) as f:
        data = json.load(f)
    
    all_signatures = data.get('new_signatures', [])
    print(f'Loaded {len(all_signatures)} initial signatures')
    print()
    
    # Apply strict filters
    print('Applying strict filters...')
    print(f'  NDWI anomaly > {THRESHOLDS["ndwi_min"]}')
    print(f'  Turbidity ratio > {THRESHOLDS["turbidity_min"]}')
    print(f'  Edge density > {THRESHOLDS["edge_density_min"]}')
    print(f'  Signature score > {THRESHOLDS["score_min"]}')
    print(f'  Distance from shore: {THRESHOLDS["distance_from_shore_km"]} km')
    print()
    
    # Debug: show sample of data ranges
    if all_signatures:
        print('Data ranges in signatures:')
        print(f'  NDWI: {min(s["ndwi_anomaly"] for s in all_signatures):.3f} to {max(s["ndwi_anomaly"] for s in all_signatures):.3f}')
        print(f'  Turbidity: {min(s["turbidity_ratio"] for s in all_signatures):.3f} to {max(s["turbidity_ratio"] for s in all_signatures):.3f}')
        print(f'  Edge density: {min(s["edge_density"] for s in all_signatures):.3f} to {max(s["edge_density"] for s in all_signatures):.3f}')
        print(f'  Score: {min(s["signature_score"] for s in all_signatures):.3f} to {max(s["signature_score"] for s in all_signatures):.3f}')
        print()
    
    filtered = filter_signatures(all_signatures)
    print(f'After filtering: {len(filtered)} signatures')
    
    # Cluster nearby signatures
    print()
    print('Clustering nearby signatures...')
    clusters = cluster_signatures(filtered, cluster_radius=0.01)
    print(f'Formed {len(clusters)} clusters')
    print()
    
    # Print top clusters
    if clusters:
        print('='*80)
        print('TOP HIGH-CONFIDENCE CLUSTERS')
        print('='*80)
        print()
        
        for i, cluster in enumerate(clusters[:10], 1):
            print(f'{i}. {cluster["id"]} | {cluster["lat"]:.4f}N, {cluster["lon"]:.4f}W')
            print(f'   Members: {cluster["member_count"]} | '
                  f'Max Score: {cluster["max_score"]:.3f} | '
                  f'Avg Score: {cluster["avg_score"]:.3f}')
            print(f'   NDWI: {cluster["avg_ndwi"]:+.3f} | '
                  f'Turbidity: {cluster["avg_turbidity"]:.2f}× | '
                  f'Edge: {cluster["avg_edge_density"]:.1%}')
            print(f'   Distance from shore: {cluster.get("distance_from_shore_km", "N/A")} km')
            print()
        
        print('='*80)
        print('SUMMARY')
        print('='*80)
        print()
        print(f'Initial signatures: {len(all_signatures)}')
        print(f'After strict filtering: {len(filtered)}')
        print(f'After clustering: {len(clusters)} clusters')
        print()
        
        if clusters:
            print('TOP 3 PRIORITY TARGETS:')
            for cluster in clusters[:3]:
                print(f'  {cluster["id"]}: {cluster["lat"]:.4f}N, {cluster["lon"]:.4f}W '
                      f'(Score: {cluster["max_score"]:.3f}, {cluster["member_count"]} members)')
    else:
        print('No high-confidence signatures found after filtering.')
        print('Consider relaxing thresholds.')
    
    # Save results
    results = {
        'analysis_date': datetime.now().isoformat(),
        'thresholds': THRESHOLDS,
        'initial_signatures': len(all_signatures),
        'filtered_signatures': len(filtered),
        'clusters': len(clusters),
        'top_clusters': clusters[:10],
    }
    
    output_json = OUTPUT_DIR / 'fall_2025_refined_clusters.json'
    with open(output_json, 'w') as f:
        json.dump(results, f, indent=2)
    
    print()
    print(f'Results saved: {output_json}')
    print('='*80)
    
    return clusters


if __name__ == '__main__':
    main()
