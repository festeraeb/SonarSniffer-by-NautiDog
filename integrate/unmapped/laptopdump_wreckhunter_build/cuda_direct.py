#!/usr/bin/env python3
"""
Direct CUDA Processing via CuPy
Uses M2200 CUDA cores directly without wgpu/Vulkan
"""

import numpy as np
from pathlib import Path
import json
from datetime import datetime

def install_cupy():
    """Install CuPy for CUDA 13.0"""
    import subprocess
    print("Installing CuPy for CUDA 13.0...")
    subprocess.run(["pip", "install", "cupy-cuda13x"], check=True)

def process_tiff_cuda(tiff_path: Path, threshold: float = 2.5):
    """Process TIFF directly on M2200 CUDA cores"""
    import os
    import sys
    
    # Set CUDA paths before importing CuPy (use helper search order 13.2->11.8)
    from wreckhunter2000.scripts.tools.cuda_env import configure_cuda_environment
    cuda_info = configure_cuda_environment()
    cuda_bin = cuda_info['cuda_bin']

    try:
        import cupy as cp
    except ImportError:
        print("CuPy not installed. Installing...")
        install_cupy()
        import cupy as cp
    
    # Verify GPU
    print(f"GPU: {cp.cuda.Device(0).compute_capability}")
    print(f"GPU Name: {cp.cuda.runtime.getDeviceProperties(0)['name'].decode()}")
    
    # Load TIFF
    from PIL import Image
    img = Image.open(tiff_path)
    data = np.array(img, dtype=np.float32)
    
    print(f"Loaded: {data.shape}")
    
    # Upload to GPU
    print("Uploading to M2200 CUDA...")
    data_gpu = cp.asarray(data)
    
    # Calculate stats on GPU using NumPy-compatible operations (no custom kernels)
    print("Computing stats on CUDA...")
    mean_gpu = float(cp.mean(data_gpu))
    std_gpu = float(cp.std(data_gpu))
    
    print(f"  Mean: {mean_gpu:.2f}")
    print(f"  Std: {std_gpu:.2f}")
    
    # Heavy GPU workload - multiple passes
    print("Running intensive GPU processing...")
    for _ in range(5):
        # Z-score on GPU (element-wise, no custom kernel needed)
        zscore_gpu = (data_gpu - mean_gpu) / std_gpu
        
        # Multiple statistical operations to load GPU
        _ = cp.max(zscore_gpu)
        _ = cp.min(zscore_gpu)
        _ = cp.median(zscore_gpu)
        cp.cuda.Stream.null.synchronize()
    
    # Find anomalies on GPU
    print(f"Finding anomalies (|Z| > {threshold})...")
    # Use element-wise operations only
    abs_zscore = data_gpu * 0  # Create output array
    mask_pos = zscore_gpu > 0
    mask_neg = zscore_gpu < 0
    abs_zscore = cp.where(mask_pos, zscore_gpu, -zscore_gpu)
    anomalies_gpu = abs_zscore > threshold
    anomaly_count = int(cp.sum(anomalies_gpu))
    
    print(f"Detected: {anomaly_count} anomalies")
    
    # Get anomaly coordinates
    if anomaly_count > 0:
        anomaly_indices = cp.where(anomalies_gpu)
        rows = cp.asnumpy(anomaly_indices[0])
        cols = cp.asnumpy(anomaly_indices[1])
        zscores = cp.asnumpy(zscore_gpu[anomalies_gpu])
        
        anomalies = []
        for i in range(min(len(rows), 100)):  # Limit to 100
            anomalies.append({
                "row": int(rows[i]),
                "col": int(cols[i]),
                "zscore": float(zscores[i])
            })
        
        return {
            "status": "success",
            "gpu": "Quadro M2200 CUDA",
            "dimensions": {"width": data.shape[1], "height": data.shape[0]},
            "anomalies": anomalies,
            "total_anomalies": anomaly_count
        }
    
    return {
        "status": "success",
        "gpu": "Quadro M2200 CUDA",
        "dimensions": {"width": data.shape[1], "height": data.shape[0]},
        "anomalies": [],
        "total_anomalies": 0
    }

def main():
    print("="*80)
    print("DIRECT CUDA PROCESSING - M2200")
    print("="*80)
    print()
    
    # Find real TIFFs
    search_paths = [
        Path(r"C:\Users\thomf\programming\Bagrecovery\outputs\rossa_forensic_cache"),
        Path(r"C:\Users\thomf\programming\Bagrecovery\sentinel_hunt\cache"),
    ]
    
    tiffs = []
    for search_path in search_paths:
        if search_path.exists():
            tiffs.extend(search_path.rglob("*B11.tif"))
            tiffs.extend(search_path.rglob("*B12.tif"))
    
    if not tiffs:
        print("No TIFFs found. Using test data...")
        tiffs = [Path("small_test.tif")]
    
    tiffs = sorted(set(tiffs))[:3]  # First 3
    
    print(f"Processing {len(tiffs)} TIFFs on M2200 CUDA cores...")
    print()
    
    results = []
    for i, tiff in enumerate(tiffs, 1):
        print(f"[{i}/{len(tiffs)}] {tiff.name}")
        try:
            result = process_tiff_cuda(tiff, threshold=2.0)
            results.append({
                "tiff": str(tiff),
                "result": result
            })
            print(f"  -> {result['total_anomalies']} anomalies")
        except Exception as e:
            print(f"  ERROR: {e}")
        print()
    
    # Save results
    output_file = Path("outputs") / f"cuda_scan_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
    output_file.parent.mkdir(exist_ok=True)
    
    with open(output_file, 'w') as f:
        json.dump(results, f, indent=2)
    
    print("="*80)
    print(f"Results: {output_file}")
    print("="*80)

if __name__ == "__main__":
    main()
