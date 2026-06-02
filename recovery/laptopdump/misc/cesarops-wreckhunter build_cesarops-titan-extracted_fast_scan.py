#!/usr/bin/env python3
"""
Fast TIFF Processing - NumPy CPU
Works without GPU compute requirements
"""

import numpy as np
from pathlib import Path
import json
from datetime import datetime
from PIL import Image

def process_tiff_fast(tiff_path: Path, threshold: float = 2.5):
    """Process TIFF with NumPy (CPU)"""
    
    # Load TIFF
    img = Image.open(tiff_path)
    data = np.array(img, dtype=np.float32)
    
    print(f"  Loaded: {data.shape}")
    
    # Calculate stats
    mean_val = np.mean(data)
    std_val = np.std(data)
    
    print(f"  Mean: {mean_val:.2f}, Std: {std_val:.2f}")
    
    # Z-score
    zscore = (data - mean_val) / std_val
    
    # Find anomalies
    anomalies_mask = np.abs(zscore) > threshold
    anomaly_count = np.sum(anomalies_mask)
    
    print(f"  Anomalies: {anomaly_count}")
    
    # Get coordinates
    if anomaly_count > 0:
        rows, cols = np.where(anomalies_mask)
        zscores = zscore[anomalies_mask]
        
        anomalies = []
        for i in range(min(len(rows), 100)):
            anomalies.append({
                "row": int(rows[i]),
                "col": int(cols[i]),
                "zscore": float(zscores[i])
            })
        
        return {
            "status": "success",
            "processor": "NumPy CPU",
            "dimensions": {"width": data.shape[1], "height": data.shape[0]},
            "anomalies": anomalies,
            "total_anomalies": int(anomaly_count)
        }
    
    return {
        "status": "success",
        "processor": "NumPy CPU",
        "dimensions": {"width": data.shape[1], "height": data.shape[0]},
        "anomalies": [],
        "total_anomalies": 0
    }

def main():
    print("="*80)
    print("FAST TIFF PROCESSING - NumPy CPU")
    print("="*80)
    print()
    
    # Find TIFFs
    search_paths = [
        Path(r"C:\Users\thomf\programming\Bagrecovery\outputs\rossa_forensic_cache"),
        Path(r"C:\Users\thomf\programming\Bagrecovery\sentinel_hunt\cache"),
    ]
    
    tiffs = []
    for search_path in search_paths:
        if search_path.exists():
            tiffs.extend(search_path.rglob("*B11.tif"))
            tiffs.extend(search_path.rglob("*B12.tif"))
    
    tiffs = sorted(set(tiffs))[:10]  # First 10
    
    print(f"Processing {len(tiffs)} TIFFs...")
    print()
    
    results = []
    for i, tiff in enumerate(tiffs, 1):
        print(f"[{i}/{len(tiffs)}] {tiff.name}")
        try:
            result = process_tiff_fast(tiff, threshold=2.0)
            results.append({
                "tiff": str(tiff),
                "result": result
            })
        except Exception as e:
            print(f"  ERROR: {e}")
        print()
    
    # Save
    output_file = Path("outputs") / f"fast_scan_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
    output_file.parent.mkdir(exist_ok=True)
    
    with open(output_file, 'w') as f:
        json.dump(results, f, indent=2)
    
    print("="*80)
    print(f"Results: {output_file}")
    print("="*80)

if __name__ == "__main__":
    main()
