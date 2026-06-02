"""
gpu_chunked_processor.py

GPU processor with SMART chunking to avoid TDR timeout.
Uses 2048x2048 chunks (large enough for speed, small enough for TDR).

Processes full 10980x10980 images in ~60-90 seconds on M2200.
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
    print('[!] CUDA not available')

# Smart chunking
CHUNK_SIZE = 2048  # Large chunks for speed, small enough for TDR
OVERLAP = 64       # Overlap to avoid edge artifacts

# ── GPU Processing ───────────────────────────────────────────────────────────

def gpu_process_chunk(chunk: torch.Tensor, num_scales: int = 4):
    """Process a single chunk on GPU."""
    
    if chunk.dim() == 2:
        chunk = chunk.unsqueeze(0).unsqueeze(0)
    
    batch, channels, height, width = chunk.shape
    all_details = []
    
    coarse = chunk
    for scale in range(num_scales):
        downsample = 2 ** (scale + 1)
        sigma = downsample * 0.5
        
        # Gaussian blur
        kernel_size = min(int(6 * sigma + 1), 31)
        if kernel_size % 2 == 0:
            kernel_size += 1
        
        x = torch.arange(kernel_size, device=DEVICE, dtype=torch.float32) - kernel_size // 2
        kernel_1d = torch.exp(-x**2 / (2 * sigma**2))
        kernel_1d = kernel_1d / kernel_1d.sum()
        kernel_2d = torch.outer(kernel_1d, kernel_1d).unsqueeze(0).unsqueeze(0)
        
        padding = kernel_size // 2
        blurred = torch.nn.functional.conv2d(coarse, kernel_2d, padding=padding)
        detail = coarse - blurred
        
        # Gradient magnitude
        grad_x = torch.abs(detail[:, :, :, :-1] - detail[:, :, :, 1:])
        grad_y = torch.abs(detail[:, :, :-1, :] - detail[:, :, 1:, :])
        
        min_h = min(grad_x.shape[2], grad_y.shape[2])
        min_w = min(grad_x.shape[3], grad_y.shape[3])
        grad_mag = grad_x[:, :, :min_h, :min_w] + grad_y[:, :, :min_h, :min_w]
        
        all_details.append(grad_mag)
        coarse = blurred
    
    return all_details


def process_full_tiff_gpu(input_path: Path, output_dir: Path):
    """Process full TIFF with smart GPU chunking."""
    
    print('='*70)
    print('GPU CHUNKED PROCESSOR (TDR-Safe)')
    print('='*70)
    print()
    print(f'Input: {input_path.name}')
    print(f'GPU: {GPU_NAME}')
    print(f'Chunk size: {CHUNK_SIZE}x{CHUNK_SIZE}')
    print()
    
    # Load entire TIFF
    print('Loading full image...')
    start = time.time()
    
    with rasterio.open(input_path) as src:
        data = src.read(1).astype(np.float32)
        height, width = data.shape
        data = (data - data.min()) / (data.max() - data.min() + 1e-8)
    
    load_time = time.time() - start
    print(f'  Loaded {height}x{width} ({height*width/1e6:.1f}M pixels) in {load_time:.2f}s')
    print()
    
    # Process in chunks
    print('Processing with GPU chunks...')
    start = time.time()
    
    all_anomalies = []
    chunk_id = 0
    
    for y in range(0, height, CHUNK_SIZE - OVERLAP):
        for x in range(0, width, CHUNK_SIZE - OVERLAP):
            # Extract chunk with overlap
            y_end = min(y + CHUNK_SIZE, height)
            x_end = min(x + CHUNK_SIZE, width)
            
            chunk_data = data[y:y_end, x:x_end]
            chunk_gpu = torch.from_numpy(chunk_data).to(DEVICE)
            
            # Process chunk
            details = gpu_process_chunk(chunk_gpu)
            
            # Detect anomalies in this chunk
            for scale_idx, detail in enumerate(details):
                magnitude = detail[0, 0]
                threshold = magnitude.mean() + 2 * magnitude.std()
                mask = magnitude > threshold
                coords = torch.nonzero(mask, as_tuple=False)
                
                for coord in coords:
                    row, col = coord.tolist()
                    # Convert to global coordinates
                    global_row = y + row
                    global_col = x + col
                    mag = magnitude[row, col].item()
                    all_anomalies.append((scale_idx, 0, global_row, global_col, mag))
            
            chunk_id += 1
            
            # Progress update every 10 chunks
            if chunk_id % 10 == 0:
                elapsed = time.time() - start
                chunks_done = chunk_id
                total_chunks = ((height - OVERLAP) // (CHUNK_SIZE - OVERLAP) + 1) * ((width - OVERLAP) // (CHUNK_SIZE - OVERLAP) + 1)
                print(f'  Chunk {chunks_done}/{total_chunks} ({100*chunks_done/total_chunks:.0f}%) - {elapsed:.1f}s')
            
            # Clear GPU memory
            del chunk_gpu
            del details
            torch.cuda.empty_cache()
    
    process_time = time.time() - start
    
    # Sort by magnitude
    all_anomalies.sort(key=lambda x: -x[4])
    
    print(f'  Processed in {process_time:.2f}s')
    print(f'  Throughput: {height*width/process_time/1e6:.2f}M pixels/sec')
    print(f'  Anomalies detected: {len(all_anomalies)}')
    print()
    
    # Save results
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    output_json = output_dir / f'{input_path.stem}_anomalies_gpu_chunked_{timestamp}.json'
    
    with open(output_json, 'w') as f:
        json.dump({
            'input_file': input_path.name,
            'size': [height, width],
            'load_time_sec': load_time,
            'process_time_sec': process_time,
            'throughput_mpix_sec': height*width/process_time/1e6,
            'gpu': GPU_NAME,
            'chunk_size': CHUNK_SIZE,
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
        print('Usage: python gpu_chunked_processor.py <input.tif> [output_dir]')
        sys.exit(1)
    
    input_path = Path(sys.argv[1])
    output_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path('outputs/gpu_chunked')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    if not input_path.exists():
        print(f'[!] File not found: {input_path}')
        sys.exit(1)
    
    process_full_tiff_gpu(input_path, output_dir)
