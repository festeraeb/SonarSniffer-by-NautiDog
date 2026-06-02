"""
huron_detect_and_fuse.py - AI Callable

Detect bright targets per available sensor/tile and fuse multi-sensor hits.
Now with AI-adjustable sensitivity and global controls.

IMPORT AS LIBRARY:
    from huron_detect_and_fuse import detect_on_raster, fuse_detections, DetectionSettings
    
    settings = DetectionSettings(sensitivity=2.5, min_size=10)
    detections = detect_on_raster('path/to/tiff.tif', settings)
    fused = fuse_detections(all_detections, settings)

CLI USAGE:
    python huron_detect_and_fuse.py --sensitivity 2.5 --lake huron --target freighter

Produces:
- outputs/huron/detections_{sensor}.json
- outputs/huron/fused_detections.json
"""

import os
import sys
import json
from pathlib import Path
import numpy as np
import rasterio
from rasterio.windows import Window
from scipy import ndimage
import argparse

# Import global controls
sys.path.insert(0, str(Path(__file__).parent.parent))
from global_controls import GlobalScannerSettings, parse_args, apply_args_to_settings

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / 'data' / 'sentinel' / 'huron'
OUT = ROOT / 'outputs' / 'huron'
OUT.mkdir(parents=True, exist_ok=True)


class DetectionSettings:
    """AI-callable detection parameters"""
    
    def __init__(self, sensitivity=3.0, min_size=10, fuse_distance_m=50):
        self.sensitivity = sensitivity  # Sigma threshold
        self.min_size = min_size        # Minimum pixels per detection
        self.fuse_distance_m = fuse_distance_m  # Distance for multi-sensor fusion
    
    @classmethod
    def from_global(cls, global_settings: GlobalScannerSettings):
        """Create from global scanner settings"""
        return cls(
            sensitivity=global_settings.sensitivity,
            min_size=10,
            fuse_distance_m=50
        )
    
    def get_threshold(self, mean, std):
        """Calculate detection threshold"""
        return mean + self.sensitivity * std


def detect_on_raster(path, settings=None):
    """
    Detect anomalies on single raster with adjustable sensitivity.
    
    Args:
        path: Path to raster file
        settings: DetectionSettings with sensitivity, min_size
    
    Returns:
        List of detection dicts
    """
    if settings is None:
        settings = DetectionSettings()
    
    with rasterio.open(path) as src:
        arr = src.read(1).astype('float32')
        
        # Calculate stats
        m = np.nanmean(arr)
        s = np.nanstd(arr)
        thr = settings.get_threshold(m, s)
        
        # Detect
        mask = arr > thr
        
        # Remove tiny objects
        label, n = ndimage.label(mask)
        sizes = ndimage.sum(mask, label, range(1, n+1))
        good = [i+1 for i,siz in enumerate(sizes) if siz >= settings.min_size]
        
        detections = []
        for lab in good:
            coords = np.where(label==lab)
            r = int(np.mean(coords[0]))
            c = int(np.mean(coords[1]))
            
            # Map to geographic
            cx, cy = src.transform * (c + 0.5, r + 0.5)
            
            detections.append({
                'row': r,
                'col': c,
                'x': float(cx),
                'y': float(cy),
                'size': int(np.sum(label==lab)),
                'zscore': float((arr[r,c] - m) / s),
                'file': str(path),
            })
        
        return detections


def fuse_detections(all_detections, settings=None):
    """
    Fuse multi-sensor detections by spatial proximity.
    
    Args:
        all_detections: Dict of sensor -> list of detections
        settings: DetectionSettings with fuse_distance_m
    
    Returns:
        List of fused detection clusters
    """
    if settings is None:
        settings = DetectionSettings()
    
    # Flatten all points
    all_pts = []
    for sensor, dets in all_detections.items():
        for d in dets:
            all_pts.append({
                'sensor': sensor,
                'x': d['x'],
                'y': d['y'],
                'size': d['size'],
                'zscore': d.get('zscore', 0)
            })
    
    # Cluster by proximity
    fused = []
    used = [False] * len(all_pts)
    
    for i, p in enumerate(all_pts):
        if used[i]:
            continue
        
        group = [p]
        used[i] = True
        
        for j, q in enumerate(all_pts):
            if used[j]:
                continue
            
            dx = p['x'] - q['x']
            dy = p['y'] - q['y']
            
            if np.hypot(dx, dy) < settings.fuse_distance_m:
                group.append(q)
                used[j] = True
        
        fused.append({
            'count': len(group),
            'sensors': list(set([m['sensor'] for m in group])),
            'members': group,
            'x': float(np.mean([m['x'] for m in group])),
            'y': float(np.mean([m['y'] for m in group])),
            'avg_zscore': float(np.mean([m['zscore'] for m in group])),
            'max_zscore': float(max([m['zscore'] for m in group])),
        })
    
    # Sort by confidence (multi-sensor + high zscore)
    fused.sort(key=lambda x: (x['count'], x['max_zscore']), reverse=True)
    
    return fused


def process_all(settings=None):
    """
    Process all sensors and fuse detections.
    
    Args:
        settings: DetectionSettings
    """
    if settings is None:
        settings = DetectionSettings()
    
    print('='*70)
    print('HURON MULTI-SENSOR DETECTION & FUSION')
    print('='*70)
    print(f'Sensitivity: {settings.sensitivity}')
    print(f'Min size: {settings.min_size} pixels')
    print(f'Fuse distance: {settings.fuse_distance_m}m')
    print()
    
    sensors = {}
    
    for f in SRC.glob('*'):
        if f.suffix.lower() in ('.tif', '.tiff', '.jp2'):
            sensor = f.stem.split('_')[0]
            dets = detect_on_raster(f, settings)
            
            sensors.setdefault(sensor, []).append({
                'file': str(f.relative_to(ROOT)),
                'detections': dets
            })
            
            # Write per-file detections
            outf = OUT / f"detections_{f.stem}.json"
            with open(outf, 'w') as fh:
                json.dump({
                    'file': str(f),
                    'detections': dets,
                    'settings': settings.__dict__
                }, fh, indent=2)
            
            print(f'  [{sensor}] {f.name}: {len(dets)} detections')
    
    # Fuse
    print()
    print('Fusing multi-sensor detections...')
    
    # Flatten for fusion
    all_dets = {}
    for sensor, items in sensors.items():
        for it in items:
            all_dets.setdefault(sensor, []).extend(it['detections'])
    
    fused = fuse_detections(all_dets, settings)
    
    with open(OUT / 'fused_detections.json', 'w') as fh:
        json.dump({
            'fused': fused,
            'total_sources': len(sensors),
            'settings': settings.__dict__,
            'processed_at': __import__('datetime').datetime.now().isoformat()
        }, fh, indent=2)
    
    print(f'  Wrote {len(fused)} fused detections')
    print(f'  Output: {OUT / "fused_detections.json"}')
    print('='*70)
    
    return fused


if __name__ == '__main__':
    # Parse CLI
    parser = argparse.ArgumentParser(description='Huron Detect & Fuse')
    parser.add_argument('--sensitivity', '-s', type=float, default=3.0,
                       help='Detection sensitivity (default: 3.0)')
    parser.add_argument('--lake', '-l', type=str, default='huron',
                       help='Lake preset (default: huron)')
    parser.add_argument('--target', '-t', type=str, default=None,
                       help='Target preset')
    parser.add_argument('--min-size', type=int, default=10,
                       help='Minimum detection size (default: 10)')
    parser.add_argument('--fuse-distance', type=float, default=50,
                       help='Fusion distance in meters (default: 50)')
    parser.add_argument('--config', type=str, default=None,
                       help='Load settings from JSON')
    parser.add_argument('--source', type=str, default=None,
                       help='Source directory (default: data/sentinel/huron)')
    
    args = parser.parse_args()
    
    # Apply global settings
    global_args = argparse.Namespace(
        sensitivity=args.sensitivity,
        lake=args.lake,
        target=args.target,
        scales=4,
        angles=16,
        chunk_size=512,
        config=args.config,
        save_config=None
    )
    global_settings = apply_args_to_settings(global_args)
    
    # Create detection settings
    settings = DetectionSettings.from_global(global_settings)
    settings.min_size = args.min_size
    settings.fuse_distance_m = args.fuse_distance
    
    # Check source exists
    if not SRC.exists():
        print(f'No source tiles at {SRC}')
        raise SystemExit(1)
    
    # Process
    process_all(settings)
