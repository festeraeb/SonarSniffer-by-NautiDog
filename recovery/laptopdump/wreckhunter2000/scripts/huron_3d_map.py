"""
Create a simple 3D point cloud from fused detections using band ratios as a pseudo-depth.

This is an approximate visualisation only.

Usage: python scripts/huron_3d_map.py
"""
import json
from pathlib import Path
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'outputs' / 'huron'
FUSED = OUT / 'fused_detections.json'

def make_ply(points, path):
    # points: list of (x,y,z)
    with open(path, 'w') as fh:
        fh.write('ply\n')
        fh.write('format ascii 1.0\n')
        fh.write(f'element vertex {len(points)}\n')
        fh.write('property float x\nproperty float y\nproperty float z\n')
        fh.write('end_header\n')
        for x,y,z in points:
            fh.write(f"{x} {y} {z}\n")

def main():
    if not FUSED.exists():
        print('No fused detections found at', FUSED)
        return
    data = json.loads(FUSED.read_text())
    pts = []
    for f in data.get('fused', []):
        x = f['x']; y = f['y']; count = f['count']
        # pseudo-depth: inversely proportional to count (more sensors -> likely shallower?)
        z = max(0.1, 10.0 / max(1, count))
        # jitter to create a small patch
        for i in range(count*2):
            pts.append((x + np.random.randn()*0.5, y + np.random.randn()*0.5, z + np.random.randn()*0.1))
    ply = OUT / 'pointcloud.ply'
    make_ply(pts, ply)
    print('Wrote', ply)

if __name__ == '__main__':
    main()
