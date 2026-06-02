"""
signature_audit.py - Target Identification for Gilcher vs Parnell

Focuses on:
- Linearity (Gilcher): 300ft steel hull = 9-10 pixel line
- Point-Mass (Parnell): Boiler/Engine block = concentrated hot/cold spot

Uses GPU acceleration for Z-score analysis on precise anomaly windows.
"""

import numpy as np
import rasterio
from rasterio.windows import Window
from pathlib import Path
import json

# Force NumPy (CuPy needs CUDA 11.2+ which isn't configured)
cp = np
HAS_CUPY = False
print('[!] Using CPU (NumPy) for signature audit')


class UniversalCoordSync:
    """Coordinate synchronization for lat/lon to pixel conversion"""
    
    def __init__(self, tiff_path):
        with rasterio.open(tiff_path) as src:
            self.transform = src.transform
            self.crs = src.crs
            self.bounds = src.bounds
    
    def latlon_to_pixel(self, lat, lon):
        """Convert WGS84 lat/lon to pixel row/col"""
        col, row = ~self.transform * (lon, lat)
        return int(row), int(col)


def signature_audit(tiff_path, target_lat, target_lon, window_size=50):
    """
    Audit a specific target location for signature type.
    
    Args:
        tiff_path: Path to GeoTIFF
        target_lat: Target latitude (WGS84)
        target_lon: Target longitude (WGS84)
        window_size: Pixel window size (default 50 = 500m x 500m for 10m resolution)
    
    Returns:
        dict with signature type and metrics
    """
    print(f'\n[SIGNATURE AUDIT]')
    print(f'  Target: {target_lat:.6f}°N, {target_lon:.6f}°W')
    print(f'  TIFF: {Path(tiff_path).name}')
    
    sync = UniversalCoordSync(tiff_path)
    target_row, target_col = sync.latlon_to_pixel(target_lat, target_lon)
    print(f'  Pixel: row={target_row}, col={target_col}')
    
    with rasterio.open(tiff_path) as src:
        half_window = window_size // 2
        row_off = max(0, target_row - half_window)
        col_off = max(0, target_col - half_window)
        row_off = min(row_off, src.height - window_size)
        col_off = min(col_off, src.width - window_size)
        
        window = Window(col_off, row_off, window_size, window_size)
        window_data = src.read(1, window=window)
    
    print(f'  Window: {window_size}x{window_size} pixels')
    
    gpu_data = cp.array(window_data).astype('float32')
    valid_mask = cp.isfinite(gpu_data) & (gpu_data != 0)
    valid_data = gpu_data[valid_mask]
    
    if len(valid_data) < 10:
        return {'status': 'INSUFFICIENT_DATA'}
    
    mean_val = float(cp.mean(valid_data))
    std_val = float(cp.std(valid_data))
    
    if std_val < 1e-6:
        return {'status': 'NO_VARIATION'}
    
    z_map = (gpu_data - mean_val) / std_val
    hits = cp.argwhere(cp.abs(z_map) > 1.5)
    
    if HAS_CUPY:
        hits = cp.asnumpy(hits)
        z_map = cp.asnumpy(z_map)
    
    num_hits = len(hits)
    print(f'  Anomalies: {num_hits} pixels (|Z| > 1.5)')
    
    if num_hits == 0:
        return {'status': 'NO_ANOMALY', 'mean': mean_val, 'std': std_val}
    
    result = {
        'target_lat': target_lat,
        'target_lon': target_lon,
        'num_anomalies': num_hits,
        'max_zscore': float(np.max(np.abs(z_map))),
    }
    
    # GILCHER TEST: Linear Hull (300ft / 9-10 pixels)
    if 5 <= num_hits <= 15:
        rows = hits[:, 0]
        cols = hits[:, 1]
        
        if len(rows) > 1:
            A = np.vstack([cols, np.ones(len(cols))]).T
            m, c = np.linalg.lstsq(A, rows, rcond=None)[0]
            distances = np.abs(m * cols - rows + c) / np.sqrt(m**2 + 1)
            linearity = float(np.mean(distances))
            
            result['linearity'] = linearity
            result['length_pixels'] = float(np.max(np.sqrt((rows - rows[0])**2 + (cols - cols[0])**2)))
            
            if result['length_pixels'] >= 8 and linearity < 2.0:
                result['signature'] = 'GILCHER_CANDIDATE'
                result['classification'] = '300ft Steel Hull - Linear Signature'
                return result
    
    # PARNELL TEST: Point Mass (Boiler/Engine Block)
    if num_hits <= 5:
        rows = hits[:, 0]
        cols = hits[:, 1]
        centroid_row = np.mean(rows)
        centroid_col = np.mean(cols)
        spread = float(np.mean(np.sqrt((rows - centroid_row)**2 + (cols - centroid_col)**2)))
        
        result['spread'] = spread
        
        if spread < 3.0:
            result['signature'] = 'PARNELL_CANDIDATE'
            result['classification'] = 'Wood/Iron Point Mass - Boiler/Engine Block'
            return result
    
    result['signature'] = 'GEOLOGIC_FEATURE'
    return result


def batch_audit_detections(detections_json, tiff_dir):
    """Audit all detections from straits_engine_master_report.json"""
    print('='*70)
    print('BATCH SIGNATURE AUDIT - GILCHER vs PARNELL')
    print('='*70)
    
    with open(detections_json) as f:
        report = json.load(f)
    
    detections = report.get('all_detections', [])
    high_conf = [d for d in detections if abs(d.get('zscore', 0)) > 3.0]
    
    print(f'\nTotal detections: {len(detections)}')
    print(f'High confidence (|Z| > 3.0): {len(high_conf)}')
    
    gilcher_candidates = []
    parnell_candidates = []
    
    for i, detection in enumerate(high_conf[:20], 1):
        print(f'\n[{i}/20] Auditing detection...')
        
        tiff_file = detection.get('tiff_path')
        if not tiff_file or not Path(tiff_file).exists():
            continue
        
        result = signature_audit(tiff_file, detection['lat'], detection['lon'])
        
        if result.get('signature') == 'GILCHER_CANDIDATE':
            gilcher_candidates.append({**detection, **result})
            print(f'  ✓ GILCHER: {result["classification"]}')
        elif result.get('signature') == 'PARNELL_CANDIDATE':
            parnell_candidates.append({**detection, **result})
            print(f'  ✓ PARNELL: {result["classification"]}')
    
    print('\n' + '='*70)
    print(f'Gilcher Candidates: {len(gilcher_candidates)}')
    print(f'Parnell Candidates: {len(parnell_candidates)}')
    
    output_path = Path(detections_json).parent / 'signature_audit_results.json'
    with open(output_path, 'w') as f:
        json.dump({
            'gilcher_candidates': gilcher_candidates,
            'parnell_candidates': parnell_candidates,
        }, f, indent=2)
    
    print(f'\nResults: {output_path}')
    return output_path


if __name__ == '__main__':
    import sys
    
    if len(sys.argv) > 1 and sys.argv[1] == '--batch':
        batch_audit_detections(sys.argv[2], sys.argv[3])
    else:
        result = signature_audit(sys.argv[1], float(sys.argv[2]), float(sys.argv[3]))
        print(json.dumps(result, indent=2))
