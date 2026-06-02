import json
import math
import os

# Load MB2 candidates
mb2 = json.load(open('outputs/erie_oct2015/mb2_candidates.json'))
dets = mb2['detections']

cluster_lat, cluster_lon = 42.42065, -82.34270

def haversine(lat1, lon1, lat2, lon2):
    R = 6371.0
    dlat = math.radians(lat2 - lat1)
    dlon = math.radians(lon2 - lon1)
    a = math.sin(dlat/2)**2 + math.cos(math.radians(lat1))*math.cos(math.radians(lat2))*math.sin(dlon/2)**2
    return R * 2 * math.asin(math.sqrt(a))

nearby = [d for d in dets if haversine(cluster_lat, cluster_lon, d['lat'], d['lon']) <= 5.0]

from collections import Counter
type_counts = Counter(d['type'] for d in nearby)
print("Type breakdown within 5km of MB2 cluster:")
for t, c in type_counts.most_common():
    print(f"  {t}: {c}")

print(f"\nTotal within 5km: {len(nearby)}")

# Peak z-scores by type
by_type = {}
for d in nearby:
    t = d['type']
    z = d.get('zscore', 0)
    if t not in by_type or z > by_type[t]:
        by_type[t] = z
print("\nPeak zscore by type:")
for t, z in sorted(by_type.items(), key=lambda x: -x[1]):
    print(f"  {t}: {z:.2f}")

# HC detections in MB2 zone specifically
hc_in_mb2 = [d for d in dets if d.get('type') == 'hydrocarbon' and d.get('mb2_zone')]
print(f"\nHC detections flagged mb2_zone=True: {len(hc_in_mb2)}")
if hc_in_mb2:
    print("First HC in mb2_zone:", hc_in_mb2[0])

# Check mag grid
MAG_GRID = 'magnetic_data/tier_2_aero_lowalt/local/gsc_erie_highres_grid.tif'
if os.path.exists(MAG_GRID):
    try:
        import rasterio
        import numpy as np
        with rasterio.open(MAG_GRID) as src:
            row, col = src.index(cluster_lon, cluster_lat)
            window = rasterio.windows.Window(col-50, row-50, 100, 100)
            data = src.read(1, window=window)
            nd = src.nodata
            valid = data[data != nd] if nd is not None else data.flatten()
            valid = valid[np.isfinite(valid)]
            print(f"\nMAG GRID at cluster center ({cluster_lat},{cluster_lon}):")
            print(f"  min={valid.min():.1f} max={valid.max():.1f} mean={valid.mean():.1f} nT")
            print(f"  99th pct: {np.percentile(valid, 99):.1f} nT")
            print(f"  Peak anomaly (p99-mean): {np.percentile(valid, 99)-valid.mean():.1f} nT")
    except Exception as e:
        print(f"\nMag grid error: {e}")
else:
    print(f"\nMag grid not found at {MAG_GRID}")
    # Search for it
    for root, dirs, files in os.walk('magnetic_data'):
        for f in files:
            if f.endswith('.tif'):
                print(f"  Found tif: {os.path.join(root, f)}")
        break  # just top level of each subdir
        for d in dirs:
            path = os.path.join(root, d)
            for f2 in os.listdir(path):
                if f2.endswith('.tif'):
                    print(f"  Found tif: {os.path.join(path, f2)}")
