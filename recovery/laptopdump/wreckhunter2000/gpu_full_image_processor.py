"""
gpu_full_image_processor.py

Loads FULL Sentinel-2 TIFF files directly onto GPU (no chunking).
Processes entire image with curvelets in one pass.
Takes thermal breaks every 15 minutes to prevent throttling.

Usage:
  python gpu_full_image_processor.py --input path/to/tiff --output path/to/output

Thermal Management:
  - Process for 15 minutes
  - Rest for 2-3 minutes (GPU cooldown)
  - Repeat until complete
"""

import torch
import numpy as np
import rasterio
import time
import argparse
from pathlib import Path
from datetime import datetime

# ── GPU Configuration ────────────────────────────────────────────────────────

# Check CUDA and FORCE GPU1 (Quadro M2200)
if torch.cuda.is_available():
    if torch.cuda.device_count() > 1:
        # Force GPU1 (Quadro M2200)
        torch.cuda.set_device(1)
        DEVICE = torch.device('cuda:1')
        GPU_NAME = torch.cuda.get_device_name(1)
        GPU_MEM = torch.cuda.get_device_properties(1).total_memory / 1e9
        print(f'[+] FORCED GPU1: {GPU_NAME} ({GPU_MEM:.2f} GB VRAM)')
    else:
        DEVICE = torch.device('cuda')
        GPU_NAME = torch.cuda.get_device_name(0)
        GPU_MEM = torch.cuda.get_device_properties(0).total_memory / 1e9
        print(f'[+] GPU: {GPU_NAME} ({GPU_MEM:.2f} GB VRAM)')
else:
    DEVICE = torch.device('cpu')
    GPU_NAME = 'CPU'
    GPU_MEM = 0
    print('[!] CUDA not available — using CPU')

# Thermal management
PROCESSING_INTERVAL = 15 * 60  # 15 minutes processing
COOLDOWN_INTERVAL = 3 * 60     # 3 minutes cooldown

# ── GPU Curvelets Functions ──────────────────────────────────────────────────

def gpu_curvelets_forward(image_tensor: torch.Tensor, num_scales: int = 4):
    """
    GPU-accelerated curvelets forward transform.
    
    Uses PyTorch CUDA operations for massive parallelism.
    Implements simplified FDCT using:
      1. Multi-scale Laplacian pyramid
      2. Directional decomposition via steerable filters
    """
    if image_tensor.dim() == 2:
        image_tensor = image_tensor.unsqueeze(0).unsqueeze(0)
    
    batch_size, channels, height, width = image_tensor.shape
    
    coarse = image_tensor.clone()
    detail_scales = []
    
    for scale in range(num_scales):
        # Downsample factor
        downsample = 2 ** (scale + 1)
        sigma = downsample * 0.5
        
        # Gaussian blur on GPU
        kernel_size = int(6 * sigma + 1)
        if kernel_size % 2 == 0:
            kernel_size += 1
        
        # Create 2D Gaussian kernel on GPU
        x = torch.arange(kernel_size, device=DEVICE, dtype=torch.float32) - kernel_size // 2
        kernel_1d = torch.exp(-x**2 / (2 * sigma**2))
        kernel_1d = kernel_1d / kernel_1d.sum()
        kernel_2d = torch.outer(kernel_1d, kernel_1d).unsqueeze(0).unsqueeze(0)
        
        # Apply convolution
        padding = kernel_size // 2
        blurred = torch.nn.functional.conv2d(coarse, kernel_2d, padding=padding)
        
        # High-pass detail
        detail = image_tensor - blurred if scale == 0 else coarse - blurred
        
        # Directional decomposition (simplified)
        num_directions = max(4, 16 // (2 ** scale))
        directional_bands = []
        
        for dir_idx in range(num_directions):
            angle = dir_idx * (180.0 / num_directions)
            angle_rad = torch.tensor(angle * np.pi / 180.0, device=DEVICE)
            
            # Create oriented filter via rotation
            y, x = torch.meshgrid(
                torch.arange(height, device=DEVICE) - height//2,
                torch.arange(width, device=DEVICE) - width//2,
                indexing='ij'
            )
            
            x_rot = x * torch.cos(angle_rad) + y * torch.sin(angle_rad)
            y_rot = -x * torch.sin(angle_rad) + y * torch.cos(angle_rad)
            
            # Gabor-like filter (parabolic scaling)
            sigma_x = downsample * 2
            sigma_y = downsample * 0.5
            
            gabor = torch.exp(-(x_rot**2 / (2*sigma_x**2) + y_rot**2 / (2*sigma_y**2)))
            gabor = gabor / gabor.sum()
            gabor = gabor.unsqueeze(0).unsqueeze(0)
            
            # Apply directional filter
            directional = torch.nn.functional.conv2d(detail, gabor, padding=padding)
            directional_bands.append(directional)
        
        detail_scales.append(directional_bands)
        coarse = blurred
    
    return coarse, detail_scales


def gpu_detect_anomalies(detail_scales, threshold: float = 0.05):
    """
    Detect anomalies from curvelets coefficients on GPU.
    
    Returns list of (scale, direction, row, col, magnitude) tuples.
    """
    anomalies = []
    
    for scale_idx, scale_bands in enumerate(detail_scales):
        for dir_idx, band in enumerate(scale_bands):
            # Compute magnitude
            magnitude = torch.abs(band)
            
            # Find pixels above threshold
            mask = magnitude > threshold
            
            # Get coordinates
            coords = torch.nonzero(mask, as_tuple=False)
            
            for coord in coords:
                batch, channel, row, col = coord.tolist()
                mag = magnitude[batch, channel, row, col].item()
                anomalies.append((scale_idx, dir_idx, row, col, mag))
    
    # Sort by magnitude
    anomalies.sort(key=lambda x: -x[4])
    
    return anomalies


# ── Full Image Processing ────────────────────────────────────────────────────

def process_full_tiff(input_path: Path, output_dir: Path, 
                     thermal_break: bool = True):
    """
    Process entire TIFF file on GPU with thermal management.
    
    Args:
        input_path: Path to input TIFF file
        output_dir: Output directory for results
        thermal_break: Whether to take thermal cooldown breaks
    """
    print('='*70)
    print('GPU FULL IMAGE PROCESSOR')
    print('='*70)
    print()
    print(f'Input: {input_path.name}')
    print(f'Output: {output_dir}')
    print(f'GPU: {GPU_NAME}')
    print()
    
    # Load entire TIFF into GPU memory
    print('Loading full image into GPU VRAM...')
    start_load = time.time()
    
    with rasterio.open(input_path) as src:
        # Read entire band
        data = src.read(1).astype(np.float32)
        height, width = data.shape
        
        # Normalize to 0-1
        data = (data - data.min()) / (data.max() - data.min() + 1e-8)
        
        # Move to GPU
        image_gpu = torch.from_numpy(data).to(DEVICE)
    
    load_time = time.time() - start_load
    print(f'  Loaded {height}x{width} ({height*width/1e6:.1f}M pixels)')
    print(f'  Load time: {load_time:.2f}s')
    print(f'  GPU VRAM used: {image_gpu.element_size() * image_gpu.nelement() / 1e6:.1f} MB')
    print()
    
    # Process with thermal management
    print('Starting curvelets processing...')
    print(f'Thermal breaks: Every {PROCESSING_INTERVAL//60} minutes')
    print(f'Cooldown: {COOLDOWN_INTERVAL//60} minutes')
    print()
    
    processing_start = time.time()
    last_break = processing_start
    
    # Process scales one at a time (allows thermal breaks between)
    num_scales = 4
    all_anomalies = []
    
    for scale in range(num_scales):
        # Check if thermal break needed
        if thermal_break and (time.time() - last_break) > PROCESSING_INTERVAL:
            print(f'\n[THERMAL BREAK] GPU processing for {PROCESSING_INTERVAL//60} minutes - cooling down...')
            time.sleep(COOLDOWN_INTERVAL)
            last_break = time.time()
            print(f'[RESUME] GPU cooled, resuming processing...\n')
        
        # Process this scale
        print(f'Processing scale {scale+1}/{num_scales}...')
        scale_start = time.time()
        
        # Run curvelets for this scale
        downsample = 2 ** (scale + 1)
        sigma = downsample * 0.5
        
        # Gaussian blur
        kernel_size = int(6 * sigma + 1)
        if kernel_size % 2 == 0:
            kernel_size += 1
        
        x = torch.arange(kernel_size, device=DEVICE, dtype=torch.float32) - kernel_size // 2
        kernel_1d = torch.exp(-x**2 / (2 * sigma**2))
        kernel_1d = kernel_1d / kernel_1d.sum()
        kernel_2d = torch.outer(kernel_1d, kernel_1d).unsqueeze(0).unsqueeze(0)
        
        if scale == 0:
            coarse = image_gpu.unsqueeze(0).unsqueeze(0)
            blurred = torch.nn.functional.conv2d(coarse, kernel_2d, padding=kernel_size//2)
            detail = coarse - blurred
        else:
            blurred = torch.nn.functional.conv2d(coarse, kernel_2d, padding=kernel_size//2)
            detail = coarse - blurred
        
        # Directional decomposition
        num_directions = max(4, 16 // (2 ** scale))
        directional_bands = []
        
        for dir_idx in range(num_directions):
            angle = dir_idx * (180.0 / num_directions)
            angle_rad = torch.tensor(angle * np.pi / 180.0, device=DEVICE)
            
            y, x = torch.meshgrid(
                torch.arange(height, device=DEVICE) - height//2,
                torch.arange(width, device=DEVICE) - width//2,
                indexing='ij'
            )
            
            x_rot = x * torch.cos(angle_rad) + y * torch.sin(angle_rad)
            y_rot = -x * torch.sin(angle_rad) + y * torch.cos(angle_rad)
            
            sigma_x = downsample * 2
            sigma_y = downsample * 0.5
            
            gabor = torch.exp(-(x_rot**2 / (2*sigma_x**2) + y_rot**2 / (2*sigma_y**2)))
            gabor = gabor / gabor.sum()
            gabor = gabor.unsqueeze(0).unsqueeze(0)
            
            directional = torch.nn.functional.conv2d(detail, gabor, padding=kernel_size//2)
            directional_bands.append(directional)
        
        # Detect anomalies in this scale
        for dir_idx, band in enumerate(directional_bands):
            magnitude = torch.abs(band)
            mask = magnitude > 0.05
            coords = torch.nonzero(mask, as_tuple=False)
            
            for coord in coords:
                row, col = coord[2].item(), coord[3].item()
                mag = magnitude[0, 0, row, col].item()
                all_anomalies.append((scale, dir_idx, row, col, mag))
        
        # Update coarse for next scale
        coarse = blurred
        
        scale_time = time.time() - scale_start
        print(f'  Scale {scale+1}/{num_scales} complete in {scale_time:.2f}s')
        print(f'  Anomalies detected: {len([a for a in all_anomalies if a[0] == scale])}')
        
        # Clear GPU memory
        torch.cuda.empty_cache()
    
    total_time = time.time() - processing_start
    
    # Sort anomalies by magnitude
    all_anomalies.sort(key=lambda x: -x[4])
    
    print()
    print('='*70)
    print('PROCESSING COMPLETE')
    print('='*70)
    print(f'Total time: {total_time/60:.1f} minutes')
    print(f'Throughput: {height*width/total_time/1e6:.2f}M pixels/sec')
    print(f'Total anomalies: {len(all_anomalies)}')
    print()
    
    # Save results
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    
    # Save anomalies as JSON
    import json
    output_json = output_dir / f'{input_path.stem}_anomalies_full_{timestamp}.json'
    with open(output_json, 'w') as f:
        json.dump({
            'input_file': input_path.name,
            'size': [height, width],
            'processing_time_sec': total_time,
            'throughput_mpix_sec': height*width/total_time/1e6,
            'gpu': GPU_NAME,
            'total_anomalies': len(all_anomalies),
            'top_anomalies': all_anomalies[:1000],  # Top 1000
        }, f, indent=2)
    
    print(f'Anomalies saved: {output_json}')
    print('='*70)
    
    return all_anomalies


# ── Main ──────────────────────────────────────────────────────────────────────

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='GPU Full Image Processor')
    parser.add_argument('--input', type=str, required=True, help='Input TIFF file')
    parser.add_argument('--output', type=str, default='outputs/gpu_full', help='Output directory')
    parser.add_argument('--no-breaks', action='store_true', help='Disable thermal breaks')
    
    args = parser.parse_args()
    
    input_path = Path(args.input)
    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)
    
    if not input_path.exists():
        print(f'[!] Input file not found: {input_path}')
        exit(1)
    
    process_full_tiff(input_path, output_dir, thermal_break=not args.no_breaks)
