"""
gpu_curvelets.py - AI Callable Library

GPU-Accelerated Fast Discrete Curvelet Transform (FDCT)
Using PyTorch CUDA on Quadro M2200

IMPORT AS LIBRARY:
    from gpu_curvelets import apply_curvelet, CurveletSettings
    
    # AI-adjustable parameters
    settings = CurveletSettings(scales=4, angles=16)
    coefficients = apply_curvelet(image, settings)

CLI USAGE:
    python gpu_curvelets.py image.tif --scales 4 --angles 16 --sensitivity 2.5
"""

import torch
import numpy as np
from typing import Tuple, List, Optional, Dict
import time
from pathlib import Path
import argparse

# Import global settings
import sys
sys.path.insert(0, str(Path(__file__).parent.parent))
from global_controls import GlobalScannerSettings, parse_args, apply_args_to_settings

# Check CUDA availability and FORCE GPU1 (Quadro M2200)
if torch.cuda.is_available():
    if torch.cuda.device_count() > 1:
        # Force GPU1 (Quadro M2200)
        torch.cuda.set_device(1)
        DEVICE = torch.device('cuda:1')
        print(f'[+] FORCED GPU1: {torch.cuda.get_device_name(1)}')
        print(f'    VRAM: {torch.cuda.get_device_properties(1).total_memory/1e9:.2f} GB')
    else:
        DEVICE = torch.device('cuda')
        print(f'[+] CUDA available: {torch.cuda.get_device_name(0)}')
        print(f'    VRAM: {torch.cuda.get_device_properties(0).total_memory/1e9:.2f} GB')
else:
    DEVICE = torch.device('cpu')
    print('[!] CUDA not available — using CPU')


class CurveletSettings:
    """AI-callable curvelet parameter container"""
    
    def __init__(self, scales=4, angles=16, sensitivity=3.0):
        self.scales = scales
        self.angles = angles
        self.sensitivity = sensitivity  # Detection threshold (sigma)
    
    @classmethod
    def from_global(cls, global_settings: GlobalScannerSettings):
        """Create from global scanner settings"""
        return cls(
            scales=global_settings.curvelet_scales,
            angles=global_settings.curvelet_angles,
            sensitivity=global_settings.sensitivity
        )
    
    def to_dict(self) -> Dict:
        return {
            'scales': self.scales,
            'angles': self.angles,
            'sensitivity': self.sensitivity
        }


def gaussian_filter_cuda(image: torch.Tensor, sigma: float) -> torch.Tensor:
    """
    Apply Gaussian blur on GPU using separable 1D convolutions.
    
    Args:
        image: 2D tensor on GPU (1, H, W)
        sigma: Gaussian standard deviation
    
    Returns:
        Blurred image on GPU
    """
    # Create 1D Gaussian kernel
    kernel_size = int(6 * sigma + 1)
    if kernel_size % 2 == 0:
        kernel_size += 1
    
    x = torch.arange(kernel_size, device=DEVICE, dtype=torch.float32) - kernel_size // 2
    kernel_1d = torch.exp(-x**2 / (2 * sigma**2))
    kernel_1d = kernel_1d / kernel_1d.sum()
    
    # Reshape for conv2d
    kernel_1d = kernel_1d.view(1, 1, -1, 1)
    
    # Apply separable convolution (horizontal then vertical)
    # Horizontal
    blurred = torch.nn.functional.conv2d(image.unsqueeze(0), kernel_1d, padding=(kernel_size//2, 0))
    # Vertical
    kernel_2d = kernel_1d.transpose(2, 3)
    blurred = torch.nn.functional.conv2d(blurred, kernel_2d, padding=(0, kernel_size//2))
    
    return blurred.squeeze(0)


def curvelet_forward_cuda(
    image: np.ndarray,
    num_scales: int = 4,
    num_directions: int = 16,
) -> Tuple[torch.Tensor, List[List[torch.Tensor]]]:
    """
    GPU-accelerated forward curvelet transform.
    
    Implements a simplified FDCT using:
      1. Multi-scale decomposition (Laplacian pyramid)
      2. Directional decomposition (steerable filters)
      3. All operations on GPU via PyTorch CUDA
    
    Args:
        image: 2D numpy array (H, W)
        num_scales: Number of decomposition scales (2-10)
        num_directions: Directions at finest scale (default 16)
    
    Returns:
        (coarse_scale, detail_scales)
        - coarse_scale: Low-frequency subband (GPU tensor)
        - detail_scales: List of directional subbands per scale
    """
    start_total = time.time()
    
    # Convert to torch tensor and move to GPU
    if isinstance(image, np.ndarray):
        image_tensor = torch.from_numpy(image).float().to(DEVICE)
    else:
        image_tensor = image.to(DEVICE)
    
    height, width = image_tensor.shape
    print(f'  GPU Input: {height}x{width} ({height*width/1e6:.1f}M pixels)')
    print(f'  Device: {image_tensor.device}')
    
    # Multi-scale decomposition using Laplacian pyramid
    coarse = image_tensor.clone()
    detail_scales = []
    
    scales_processed = 0
    for scale in range(num_scales):
        scale_start = time.time()
        
        # Downsample factor for this scale
        downsample = 2 ** (scale + 1)
        
        # Apply Gaussian blur at this scale
        sigma = downsample * 0.5
        blurred = gaussian_filter_cuda(image_tensor, sigma)
        
        # High-pass (detail) = original - blurred
        detail = image_tensor - blurred
        
        # Directional decomposition
        # Apply oriented filters at multiple angles
        directions = num_directions // (2 ** scale)  # Fewer directions at coarser scales
        if directions < 4:
            directions = 4
        
        directional_bands = []
        for dir_idx in range(directions):
            angle = dir_idx * (180.0 / directions)
            angle_rad = torch.tensor(angle * np.pi / 180.0, device=DEVICE)
            
            # Create oriented Gabor-like filter
            # This is a simplified curvelet directionality
            y, x = torch.meshgrid(
                torch.arange(height, device=DEVICE) - height//2,
                torch.arange(width, device=DEVICE) - width//2,
                indexing='ij'
            )
            
            # Rotate coordinates
            x_rot = x * torch.cos(angle_rad) + y * torch.sin(angle_rad)
            y_rot = -x * torch.sin(angle_rad) + y * torch.cos(angle_rad)
            
            # Gabor filter (elongated Gaussian)
            sigma_x = downsample * 2
            sigma_y = downsample * 0.5  # Parabolic scaling: width ≈ length²
            
            gabor = torch.exp(-(x_rot**2 / (2*sigma_x**2) + y_rot**2 / (2*sigma_y**2)))
            gabor = gabor / gabor.sum()
            
            # Apply directional filter via multiplication in frequency domain
            # (simplified - real curvelets use wrapping/FFT)
            detail_fft = torch.fft.fft2(detail)
            gabor_fft = torch.fft.fft2(gabor)
            directional = torch.fft.ifft2(detail_fft * gabor_fft).real
            
            directional_bands.append(directional)
        
        detail_scales.append(directional_bands)
        scales_processed += 1
        
        scale_time = time.time() - scale_start
        print(f'    Scale {scale+1}/{num_scales}: {directions} directions in {scale_time:.2f}s')
        
        # Update coarse for next scale
        coarse = blurred
    
    # Final coarse scale (low-pass)
    coarse = gaussian_filter_cuda(coarse, num_scales * 2)
    
    total_time = time.time() - start_total
    print(f'  ✓ GPU curvelets complete in {total_time:.2f}s')
    print(f'  ✓ Throughput: {height*width/total_time/1e6:.2f}M pixels/sec')
    
    return coarse, detail_scales


def detect_anomalies_cuda(
    detail_scales: List[List[torch.Tensor]],
    threshold: float = 0.1,
) -> List[Tuple[int, int, int, float]]:
    """
    Detect anomalies from curvelets coefficients on GPU.
    
    Args:
        detail_scales: Output from curvelet_forward_cuda
        threshold: Detection threshold (0-1)
    
    Returns:
        List of (scale, direction, row, col, magnitude) tuples
    """
    anomalies = []
    
    for scale_idx, scale_bands in enumerate(detail_scales):
        for dir_idx, band in enumerate(scale_bands):
            # Compute magnitude
            magnitude = torch.abs(band)
            
            # Find pixels above threshold
            mask = magnitude > threshold
            
            # Get coordinates of anomalies
            coords = torch.nonzero(mask, as_tuple=False)
            
            for coord in coords:
                row, col = coord.tolist()
                mag = magnitude[row, col].item()
                anomalies.append((scale_idx, dir_idx, row, col, mag))
    
    # Sort by magnitude (highest first)
    anomalies.sort(key=lambda x: -x[4])
    
    return anomalies


def process_satellite_scene_gpu(
    scene_path: Path,
    output_dir: Path,
    num_scales: int = 4,
    chunk_size: int = 512,  # Reduced from full image to avoid TDR timeout
) -> dict:
    """
    Process Sentinel-2 scene with GPU curvelets.
    
    Args:
        scene_path: Path to Sentinel-2 TIFF file
        output_dir: Output directory for results
        num_scales: Number of curvelet scales
    
    Returns:
        Processing result dict
    """
    import rasterio
    
    print(f'Processing {scene_path.name} on GPU...')
    
    # Load imagery
    with rasterio.open(scene_path) as src:
        data = src.read(1).astype(np.float32)
        height, width = data.shape
        
        # Normalize to 0-1
        data = (data - data.min()) / (data.max() - data.min() + 1e-8)
    
    # Process on GPU in SMALL CHUNKS to avoid TDR timeout
    print(f'  Running GPU curvelets ({num_scales} scales, {chunk_size}x{chunk_size} chunks)...')
    
    # Create output arrays
    coarse_full = np.zeros((height, width), dtype=np.float32)
    all_anomalies = []
    
    chunks_processed = 0
    start_total = time.time()
    
    for y in range(0, height, chunk_size):
        for x in range(0, width, chunk_size):
            # Extract chunk
            y_end = min(y + chunk_size, height)
            x_end = min(x + chunk_size, width)
            chunk = data[y:y_end, x:x_end]
            
            # Process chunk on GPU
            chunk_coarse, chunk_details = curvelet_forward_cuda(chunk, num_scales)
            
            # Detect anomalies in chunk
            chunk_anomalies = detect_anomalies_cuda(chunk_details, threshold=0.05)  # Lowered from 0.15
            
            # Adjust anomaly coordinates to global image coordinates
            for scale_idx, dir_idx, row, col, mag in chunk_anomalies:
                all_anomalies.append((scale_idx, dir_idx, y + row, x + col, mag))
            
            # Save coarse to full array
            coarse_full[y:y_end, x:x_end] = chunk_coarse.cpu().numpy()
            
            # Free GPU memory
            del chunk_coarse
            del chunk_details
            torch.cuda.empty_cache()
            
            chunks_processed += 1
            
            # 3-second cooldown between chunks to prevent thermal throttling
            time.sleep(3)
            
            if chunks_processed % 10 == 0:
                elapsed = time.time() - start_total
                print(f'    [{chunks_processed}] {elapsed:.1f}s - {chunk_size*chunk_size*chunks_processed/elapsed/1e6:.1f}M pixels/sec')
    
    total_time = time.time() - start_total
    print(f'  ✓ GPU curvelets complete in {total_time:.2f}s ({chunks_processed} chunks)')
    
    # Save outputs
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # Save coarse scale
    coarse_path = output_dir / f'{scene_path.stem}_coarse_gpu.npy'
    np.save(coarse_path, coarse_full)
    
    # Save anomalies
    import json
    anomaly_path = output_dir / f'{scene_path.stem}_anomalies_gpu.json'
    with open(anomaly_path, 'w') as f:
        json.dump({
            'scene': scene_path.name,
            'size': [height, width],
            'num_scales': num_scales,
            'chunk_size': chunk_size,
            'total_anomalies': len(all_anomalies),
            'top_anomalies': sorted(all_anomalies, key=lambda x: -x[4])[:100],
        }, f, indent=2)
    
    # Free GPU memory
    torch.cuda.empty_cache()
    
    result = {
        'input_file': scene_path.name,
        'size': [height, width],
        'num_scales': num_scales,
        'chunk_size': chunk_size,
        'coarse_output': str(coarse_path),
        'anomalies_detected': len(all_anomalies),
        'anomaly_output': str(anomaly_path),
        'processing_time_sec': total_time,
        'throughput_mpix_sec': height*width/total_time/1e6,
    }
    
    return result


def batch_process_sentinel2(
    cache_dir: Path,
    output_dir: Path,
) -> List[dict]:
    """
    Process all Sentinel-2 bands in cache with GPU curvelets.
    
    Args:
        cache_dir: Directory with downloaded Sentinel-2 TIFFs
        output_dir: Output directory for results
    
    Returns:
        List of processing results
    """
    print('='*70)
    print('GPU CURVELETS - SENTINEL-2 BATCH PROCESSING')
    print('='*70)
    print()
    
    # Find all TIFF files
    tif_files = list(cache_dir.glob('*.tif'))
    print(f'Found {len(tif_files)} Sentinel-2 bands')
    print(f'Processing on: {torch.cuda.get_device_name(0)}')
    print()
    
    results = []
    
    for i, tif_path in enumerate(tif_files, 1):
        print(f'[{i}/{len(tif_files)}] {tif_path.stem}')
        
        try:
            result = process_satellite_scene_gpu(tif_path, output_dir, num_scales=4)
            results.append(result)
            print(f'  ✓ Complete: {result["anomalies_detected"]} anomalies')
            print()
        except Exception as e:
            print(f'  ✗ Error: {e}')
            print()
            results.append({
                'input_file': tif_path.name,
                'error': str(e),
            })
    
    # Save summary
    import json
    summary_path = output_dir / 'gpu_curvelets_summary.json'
    with open(summary_path, 'w') as f:
        json.dump({
            'processed_at': time.strftime('%Y-%m-%d %H:%M:%S'),
            'gpu': torch.cuda.get_device_name(0),
            'total_bands': len(tif_files),
            'successful': len([r for r in results if 'error' not in r]),
            'results': results,
        }, f, indent=2)
    
    print('='*70)
    print('BATCH PROCESSING COMPLETE')
    print('='*70)
    print(f'Total bands: {len(results)}')
    print(f'Successful: {len([r for r in results if "error" not in r])}')
    print(f'Outputs: {output_dir}')
    print('='*70)
    
    return results


if __name__ == '__main__':
    # Parse CLI arguments
    parser = argparse.ArgumentParser(description='GPU Curvelet Transform')
    parser.add_argument('input', type=str, help='Input TIFF file or directory')
    parser.add_argument('--scales', type=int, default=4, help='Number of scales')
    parser.add_argument('--angles', type=int, default=16, help='Number of angles')
    parser.add_argument('--sensitivity', type=float, default=3.0, help='Detection threshold')
    parser.add_argument('--output', type=str, default='outputs/gpu_curvelets', help='Output directory')
    parser.add_argument('--lake', type=str, default='michigan', help='Lake preset')
    parser.add_argument('--target', type=str, default=None, help='Target preset')
    
    args = parser.parse_args()
    
    # Apply global settings
    global_args = argparse.Namespace(
        sensitivity=args.sensitivity,
        lake=args.lake,
        target=args.target,
        scales=args.scales,
        angles=args.angles,
        chunk_size=512,
        config=None,
        save_config=None
    )
    settings = apply_args_to_settings(global_args)
    
    # Process
    input_path = Path(args.input)
    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)
    
    if input_path.is_file():
        # Single file
        result = process_satellite_scene_gpu(
            input_path, 
            output_dir, 
            num_scales=settings.curvelet_scales
        )
        print(f"\nResult: {json.dumps(result, indent=2)}")
    elif input_path.is_dir():
        # Batch process
        batch_process_sentinel2(input_path, output_dir)
    else:
        print(f"Error: {input_path} not found")


# =============================================================================
# LIBRARY FUNCTIONS - Import in other scripts
# =============================================================================

def apply_curvelet(
    image: np.ndarray,
    settings: Optional[CurveletSettings] = None
) -> Tuple[torch.Tensor, List[List[torch.Tensor]]]:
    """
    Apply curvelet transform to image with AI-adjustable parameters.
    
    This is the main library function for other scripts to import.
    
    Args:
        image: 2D numpy array (H, W)
        settings: CurveletSettings with scales, angles, sensitivity
    
    Returns:
        (coarse_scale, detail_scales) - GPU tensors
    """
    if settings is None:
        settings = CurveletSettings()
    
    return curvelet_forward_cuda(
        image,
        num_scales=settings.scales,
        num_directions=settings.angles
    )


def detect_anomalies_from_curvelet(
    coefficients: Tuple[torch.Tensor, List[List[torch.Tensor]]],
    settings: Optional[CurveletSettings] = None
) -> List[Dict]:
    """
    Detect anomalies from curvelet coefficients.
    
    Args:
        coefficients: Output from apply_curvelet()
        settings: CurveletSettings with sensitivity threshold
    
    Returns:
        List of anomaly dicts with row, col, zscore, scale, angle
    """
    if settings is None:
        settings = CurveletSettings()
    
    coarse, details = coefficients
    
    # Calculate statistics on finest scale (most detail)
    if details and details[0]:
        finest_scale = details[0]  # First scale = finest
        
        # Combine all directional bands
        combined = torch.stack([band.abs() for band in finest_scale]).mean(dim=0)
        
        # Calculate threshold
        mean_val = float(combined.mean())
        std_val = float(combined.std())
        threshold = mean_val + settings.sensitivity * std_val
        
        # Find anomalies
        anomaly_mask = combined > threshold
        anomaly_coords = torch.where(anomaly_mask)
        
        anomalies = []
        for row, col in zip(anomaly_coords[0].cpu().numpy(), anomaly_coords[1].cpu().numpy()):
            zscore = float((combined[row, col] - mean_val) / std_val)
            anomalies.append({
                'row': int(row),
                'col': int(col),
                'zscore': zscore,
                'scale': 0,
                'angle': 0
            })
        
        return anomalies
    
    return []
