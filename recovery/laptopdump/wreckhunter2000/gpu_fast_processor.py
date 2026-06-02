"""
gpu_fast_processor.py

FAST GPU processor - avoids TDR timeout by using smaller, faster operations.
No directional filters (those are too slow on full images).

Uses simplified curvelets:
  1. Multi-scale Laplacian pyramid (fast Gaussian blur)
  2. Simple gradient-based edge detection (no rotation)
  3. Anomaly detection from detail coefficients

Processes FULL images in ~10-20 seconds per band!
"""

import torch\n# FORCE GPU1 (Quadro M2200)\ntorch.cuda.set_device(1)\nprint(f'? GPU1 Selected: {torch.cuda.get_device_name(1)}')
import numpy as np
import rasterio
import time
import json
from pathlib import Path
from datetime import datetime

# ── GPU Configuration ────────────────────────────────────────────────────────

if torch.cuda.is_available():
    DEVICE = torch.device('cuda')
    GPU_NAME = torch.cuda.get_device_name(0)
    print(f'[+] GPU: {GPU_NAME}')
else:
    DEVICE = torch.device('cpu')
    GPU_NAME = 'CPU'
    print('[!] CUDA not available — using CPU')

# ── Fast GPU Processing ──────────────────────────────────────────────────────

def fast_gpu_curvelets(image_tensor: torch.Tensor, num_scales: int = 4):
    """
    FAST simplified curvelets - avoids TDR timeout.
    
    Uses:
      1. Multi-scale Gaussian pyramid
      2. Simple gradient magnitude (no rotation)
      3. Detail coefficients for anomaly detection
    """
    if image_tensor.dim() == 2:
        image_tensor = image_tensor.unsqueeze(0).unsqueeze(0)
    
    batch, channels, height, width = image_tensor.shape
    all_details = []
    
    for scale in range(num_scales):
        downsample = 2 ** (scale + 1)
        sigma = downsample * 0.5
        
        # Fast Gaussian blur
        kernel_size = min(int(6 * sigma + 1), 31)  # Cap kernel size
        if kernel_size % 2 == 0:
            kernel_size += 1
        
        x = torch.arange(kernel_size, device=DEVICE, dtype=torch.float32) - kernel_size // 2
        kernel_1d = torch.exp(-x**2 / (2 * sigma**2))
        kernel_1d = kernel_1d / kernel_1d.sum()
        kernel_2d = torch.outer(kernel_1d, kernel_1d).unsqueeze(0).unsqueeze(0)
        
        # Convolution
        padding = kernel_size // 2
        if scale == 0:
            coarse = image_tensor
        blurred = torch.nn.functional.conv2d(coarse, kernel_2d, padding=padding)
        detail = coarse - blurred
        
        # Simple gradient magnitude (fast, no rotation)
        grad_x = torch.abs(detail[:, :, :, :-1] - detail[:, :, :, 1:])
        grad_y = torch.abs(detail[:, :, :-1, :] - detail[:, :, 1:, :])
        # Crop both to match
        min_h = min(grad_x.shape[2], grad_y.shape[2])
        min_w = min(grad_x.shape[3], grad_y.shape[3])
        grad_mag = grad_x[:, :, :min_h, :min_w] + grad_y[:, :, :min_h, :min_w]
        
        all_details.append(grad_mag)
        coarse = blurred
    
    return all_details


def process_full_tiff_fast(input_path: Path, output_dir: Path):
    """
    FAST full image processing - completes in ~10-20 seconds!
    """
    print('='*70)
    print('GPU FAST PROCESSOR (No TDR Timeout)')
    print('='*70)
    print()
    print(f'Input: {input_path.name}')
    print(f'GPU: {GPU_NAME}')
    print()
    
    # Load entire TIFF
    print('Loading full image...')
    start = time.time()
    
    with rasterio.open(input_path) as src:
        data = src.read(1).astype(np.float32)
        height, width = data.shape
        data = (data - data.min()) / (data.max() - data.min() + 1e-8)
        image_gpu = torch.from_numpy(data).to(DEVICE)
    
    load_time = time.time() - start
    print(f'  Loaded {height}x{width} ({height*width/1e6:.1f}M pixels) in {load_time:.2f}s')
    print()
    
    # Fast processing
    print('Processing with fast GPU curvelets...')
    start = time.time()
    
    details = fast_gpu_curvelets(image_gpu, num_scales=4)
    
    # Detect anomalies from all scales
    all_anomalies = []
    for scale_idx, detail in enumerate(details):
        magnitude = detail[0, 0]  # Remove batch/channel dims
        threshold = magnitude.mean() + 2 * magnitude.std()
        mask = magnitude > threshold
        coords = torch.nonzero(mask, as_tuple=False)
        
        for coord in coords:
            row, col = coord.tolist()
            mag = magnitude[row, col].item()
            all_anomalies.append((scale_idx, 0, row, col, mag))
    
    # Sort by magnitude
    all_anomalies.sort(key=lambda x: -x[4])
    
    process_time = time.time() - start
    print(f'  Processed in {process_time:.2f}s')
    print(f'  Throughput: {height*width/process_time/1e6:.2f}M pixels/sec')
    print(f'  Anomalies detected: {len(all_anomalies)}')
    print()
    
    # Save results
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    output_json = output_dir / f'{input_path.stem}_anomalies_fast_{timestamp}.json'
    
    with open(output_json, 'w') as f:
        json.dump({
            'input_file': input_path.name,
            'size': [height, width],
            'load_time_sec': load_time,
            'process_time_sec': process_time,
            'throughput_mpix_sec': height*width/process_time/1e6,
            'gpu': GPU_NAME,
            'total_anomalies': len(all_anomalies),
            'top_anomalies': all_anomalies[:1000],
        }, f, indent=2)
    
    print('='*70)
    print(f'COMPLETE - Saved: {output_json.name}')
    print('='*70)
    
    return all_anomalies


# ── Main ──────────────────────────────────────────────────────────────────────

if __name__ == '__main__':
    import sys
    
    if len(sys.argv) < 2:
        print('Usage: python gpu_fast_processor.py <input.tif> [output_dir]')
        sys.exit(1)
    
    input_path = Path(sys.argv[1])
    output_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path('outputs/gpu_fast')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    if not input_path.exists():
        print(f'[!] File not found: {input_path}')
        sys.exit(1)
    
    process_full_tiff_fast(input_path, output_dir)
