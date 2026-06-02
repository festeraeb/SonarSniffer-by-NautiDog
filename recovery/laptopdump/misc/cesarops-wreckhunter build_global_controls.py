# global_controls.py
# Master Scanner Settings - AI Callable
# All thresholds and parameters adjustable via CLI or import

import cupy as cp
import argparse
import json
from pathlib import Path

class GlobalScannerSettings:
    """
    Global settings for all CESAROPS GPU tools.
    AI can adjust these on-the-fly for different lakes/targets.
    """
    
    def __init__(self):
        # Core detection sensitivity (sigma threshold)
        # Lower = more sensitive (more false positives)
        # Higher = less sensitive (fewer false positives)
        self.sensitivity = 3.0  # Default: mean + 3*std
        
        # Curvelet transform parameters
        self.curvelet_scales = 4      # Number of decomposition scales (2-10)
        self.curvelet_angles = 16     # Directions at finest scale (4-32)
        
        # Lake-specific presets
        self.lake_presets = {
            'superior': {
                'sensitivity': 2.0,    # Deep water, need higher sensitivity
                'curvelet_scales': 5,
                'curvelet_angles': 8,  # Fewer angles to save VRAM on big tiles
                'min_temp_k': 273.0,
                'max_temp_k': 285.0,
            },
            'michigan': {
                'sensitivity': 2.5,
                'curvelet_scales': 4,
                'curvelet_angles': 16,
                'min_temp_k': 275.0,
                'max_temp_k': 295.0,
            },
            'huron': {
                'sensitivity': 2.2,
                'curvelet_scales': 4,
                'curvelet_angles': 12,
                'min_temp_k': 274.0,
                'max_temp_k': 290.0,
            },
            'erie': {
                'sensitivity': 4.5,    # Shallow, lots of "noise"
                'curvelet_scales': 3,
                'curvelet_angles': 8,
                'min_temp_k': 278.0,
                'max_temp_k': 305.0,
            },
            'ontario': {
                'sensitivity': 2.8,
                'curvelet_scales': 4,
                'curvelet_angles': 12,
                'min_temp_k': 274.0,
                'max_temp_k': 288.0,
            },
        }
        
        # Target-specific presets
        self.target_presets = {
            'andaste': {  # 310ft whaleback
                'sensitivity': 1.8,
                'min_length_ft': 250,
                'max_length_ft': 350,
                'depth_ft': 180,
            },
            'aircraft': {  # Flight 2501 DC-4
                'sensitivity': 2.0,
                'min_length_ft': 100,
                'max_length_ft': 150,
                'aluminum_ratio': 1.4,
            },
            'freighter': {  # Large steel vessels
                'sensitivity': 1.6,
                'min_length_ft': 350,
                'max_length_ft': 600,
                'min_mass_tons': 10000,
            },
            'workboat': {  # Bridge Builder X type
                'sensitivity': 2.5,
                'min_length_ft': 50,
                'max_length_ft': 150,
            },
            'monster': {  # 14000+ ton masses
                'sensitivity': 1.5,
                'min_mass_tons': 14000,
                'thermal_sink': 0.7,
            },
        }
        
        # M2200 VRAM management (4GB limit)
        self.vram_settings = {
            'chunk_size': 512,       # Process in 512x512 chunks
            'overlap_percent': 10,   # 10% overlap for edge matching (51 pixels)
            'use_streams': True,     # Use CuPy streams for async
            'max_vram_gb': 3.5,      # Leave 0.5GB for display
        }
    
    @property
    def chunk_size(self):
        return self.vram_settings['chunk_size']
    
    @property
    def overlap_percent(self):
        return self.vram_settings['overlap_percent']
    
    @property
    def overlap_pixels(self):
        return int(self.chunk_size * self.overlap_percent / 100)
    
    def update_for_lake(self, lake_name):
        """Apply lake-specific preset"""
        lake_name = lake_name.lower()
        if lake_name in self.lake_presets:
            preset = self.lake_presets[lake_name]
            self.sensitivity = preset['sensitivity']
            self.curvelet_scales = preset['curvelet_scales']
            self.curvelet_angles = preset['curvelet_angles']
            print(f"[+] Loaded preset for Lake {lake_name.capitalize()}")
            print(f"    Sensitivity: {self.sensitivity}")
            print(f"    Curvelet scales: {self.curvelet_scales}, angles: {self.curvelet_angles}")
            return True
        return False
    
    def update_for_target(self, target_type):
        """Apply target-specific preset"""
        target_type = target_type.lower()
        if target_type in self.target_presets:
            preset = self.target_presets[target_type]
            self.sensitivity = preset['sensitivity']
            print(f"[+] Loaded preset for {target_type}")
            print(f"    Sensitivity: {self.sensitivity}")
            return preset
        return {}
    
    def get_threshold(self, mean, std):
        """Calculate threshold from sensitivity"""
        return mean + self.sensitivity * std
    
    def to_dict(self):
        """Export settings as dict"""
        return {
            'sensitivity': self.sensitivity,
            'curvelet_scales': self.curvelet_scales,
            'curvelet_angles': self.curvelet_angles,
            'vram': self.vram_settings,
        }
    
    def save(self, path):
        """Save settings to JSON"""
        with open(path, 'w') as f:
            json.dump(self.to_dict(), f, indent=2)
    
    def load(self, path):
        """Load settings from JSON"""
        with open(path) as f:
            data = json.load(f)
            self.sensitivity = data.get('sensitivity', 3.0)
            self.curvelet_scales = data.get('curvelet_scales', 4)
            self.curvelet_angles = data.get('curvelet_angles', 16)


# Global instance
GLOBAL_SETTINGS = GlobalScannerSettings()


def parse_args():
    """Standard argument parser for all tools"""
    parser = argparse.ArgumentParser(description='CESAROPS GPU Scanner')
    
    parser.add_argument('--sensitivity', '-s', type=float, default=3.0,
                       help='Detection sensitivity (sigma threshold, default: 3.0)')
    parser.add_argument('--lake', '-l', type=str, default='michigan',
                       choices=['superior', 'michigan', 'huron', 'erie', 'ontario'],
                       help='Great Lake to scan (default: michigan)')
    parser.add_argument('--target', '-t', type=str, default=None,
                       choices=['andaste', 'aircraft', 'freighter', 'workboat', 'monster'],
                       help='Target type preset (default: none)')
    parser.add_argument('--scales', type=int, default=4,
                       help='Curvelet decomposition scales (default: 4)')
    parser.add_argument('--angles', type=int, default=16,
                       help='Curvelet directions (default: 16)')
    parser.add_argument('--chunk-size', type=int, default=512,
                       help='GPU chunk size for M2200 VRAM (default: 512)')
    parser.add_argument('--config', type=str, default=None,
                       help='Load settings from JSON file')
    parser.add_argument('--save-config', type=str, default=None,
                       help='Save current settings to JSON file')
    
    return parser.parse_args()


def apply_args_to_settings(args):
    """Apply CLI args to global settings"""
    if args.config:
        GLOBAL_SETTINGS.load(args.config)
        print(f"[+] Loaded config from {args.config}")
    
    if args.lake:
        GLOBAL_SETTINGS.update_for_lake(args.lake)
    
    if args.target:
        GLOBAL_SETTINGS.update_for_target(args.target)
    
    # Override with explicit args
    if args.sensitivity != 3.0:
        GLOBAL_SETTINGS.sensitivity = args.sensitivity
    if args.scales != 4:
        GLOBAL_SETTINGS.curvelet_scales = args.scales
    if args.angles != 16:
        GLOBAL_SETTINGS.curvelet_angles = args.angles
    if args.chunk_size != 512:
        GLOBAL_SETTINGS.vram_settings['chunk_size'] = args.chunk_size
    
    if args.save_config:
        GLOBAL_SETTINGS.save(args.save_config)
        print(f"[+] Saved config to {args.save_config}")
    
    return GLOBAL_SETTINGS


# =============================================================================
# TILE CHUNKING WITH OVERLAP (GEOREFERENCE-AWARE)
# =============================================================================

def generate_overlapping_tiles(image_shape, chunk_size=512, overlap_percent=10, geotransform=None):
    """
    Generate tile coordinates with overlap for seamless processing.
    PRESERVES GEOTRANSFORM for each tile so we can map back to real coordinates.

    Args:
        image_shape: (height, width) of full image
        chunk_size: Size of each tile (default 512)
        overlap_percent: Overlap percentage (default 10% = 51 pixels)
        geotransform: GDAL geotransform (6-tuple) if available

    Yields:
        dict with tile coordinates, overlap info, and GEOREFERENCE data
    """
    height, width = image_shape
    overlap = int(chunk_size * overlap_percent / 100)
    stride = chunk_size - overlap  # Step size between tiles

    tile_id = 0

    for row_start in range(0, height, stride):
        for col_start in range(0, width, stride):
            # Calculate tile bounds
            row_end = min(row_start + chunk_size, height)
            col_end = min(col_start + chunk_size, width)

            # Ensure we don't go negative on overlap
            overlap_top = overlap if row_start > 0 else 0
            overlap_left = overlap if col_start > 0 else 0
            overlap_bottom = overlap if row_end < height else 0
            overlap_right = overlap if col_end < width else 0

            # Adjust for image edges
            if row_end - row_start < chunk_size:
                row_start = max(0, row_end - chunk_size)
            if col_end - col_start < chunk_size:
                col_start = max(0, col_end - chunk_size)

            # Calculate geotransform for THIS tile
            # GDAL geotransform: (top_left_x, pixel_width, rotation, top_left_y, rotation, pixel_height)
            tile_geotransform = None
            if geotransform is not None:
                # Shift top-left corner to match tile's position
                tile_geotransform = (
                    geotransform[0] + col_start * geotransform[1],  # New top-left X
                    geotransform[1],  # Pixel width (same)
                    geotransform[2],  # Rotation (same)
                    geotransform[3] + row_start * geotransform[5],  # New top-left Y
                    geotransform[4],  # Rotation (same)
                    geotransform[5],  # Pixel height (same)
                )

            yield {
                'tile_id': tile_id,
                'row_start': row_start,
                'row_end': row_end,
                'col_start': col_start,
                'col_end': col_end,
                'overlap_top': overlap_top,
                'overlap_left': overlap_left,
                'overlap_bottom': overlap_bottom,
                'overlap_right': overlap_right,
                'width': col_end - col_start,
                'height': row_end - row_start,
                'is_edge': (row_start == 0 or col_start == 0 or
                           row_end == height or col_end == width),
                'geotransform': tile_geotransform,  # GEOREFERENCE FOR THIS TILE!
            }

            tile_id += 1


def stitch_tiles_back(tile_results, image_shape, chunk_size=512, overlap_percent=10, full_geotransform=None):
    """
    Stitch processed tiles back into full image, blending overlaps.
    PRESERVES GEOREFERENCE so we can map pixels to real coordinates.

    Args:
        tile_results: List of dicts with 'tile_info', 'data', and optional 'anomalies'
        image_shape: (height, width) of full image
        chunk_size: Size of each tile
        overlap_percent: Overlap percentage
        full_geotransform: Original image geotransform to preserve

    Returns:
        dict with stitched image and GEOREFERENCE data
    """
    import numpy as np

    height, width = image_shape
    result = np.zeros((height, width), dtype=np.float32)
    weight = np.zeros((height, width), dtype=np.float32)

    overlap = int(chunk_size * overlap_percent / 100)

    for tile in tile_results:
        row_start = tile['tile_info']['row_start']
        row_end = tile['tile_info']['row_end']
        col_start = tile['tile_info']['col_start']
        col_end = tile['tile_info']['col_end']

        # Get tile data
        data = tile['data']

        # Create weight mask (feather edges in overlap regions)
        tile_h, tile_w = data.shape
        mask = np.ones((tile_h, tile_w), dtype=np.float32)

        # Feather overlap regions
        if overlap > 0:
            # Top overlap
            if tile['tile_info']['overlap_top'] > 0:
                for i in range(overlap):
                    mask[i, :] *= (i + 1) / overlap
            # Left overlap
            if tile['tile_info']['overlap_left'] > 0:
                for j in range(overlap):
                    mask[:, j] *= (j + 1) / overlap
            # Bottom overlap
            if tile['tile_info']['overlap_bottom'] > 0:
                for i in range(overlap):
                    mask[tile_h - i - 1, :] *= (i + 1) / overlap
            # Right overlap
            if tile['tile_info']['overlap_right'] > 0:
                for j in range(overlap):
                    mask[:, tile_w - j - 1] *= (j + 1) / overlap

        # Add to result with weighting
        result[row_start:row_end, col_start:col_end] += data * mask
        weight[row_start:row_end, col_start:col_end] += mask

    # Normalize by weight
    weight[weight == 0] = 1  # Avoid division by zero
    result = result / weight

    return {
        'data': result,
        'geotransform': full_geotransform,  # Preserve for coordinate mapping
        'shape': image_shape,
    }


if __name__ == '__main__':
    args = parse_args()
    settings = apply_args_to_settings(args)
    
    print("\n=== GLOBAL SCANNER SETTINGS ===")
    print(f"Sensitivity (sigma): {settings.sensitivity}")
    print(f"Curvelet scales: {settings.curvelet_scales}")
    print(f"Curvelet angles: {settings.curvelet_angles}")
    print(f"VRAM chunk size: {settings.vram_settings['chunk_size']}")
    print(f"Overlap: {settings.overlap_percent}% ({settings.overlap_pixels} pixels)")
    print(f"Settings: {settings.to_dict()}")
    
    # Test tile generation
    print("\n=== TILE GENERATION TEST ===")
    test_shape = (3660, 3660)  # Typical Landsat tile
    chunk_size = settings.vram_settings['chunk_size']
    overlap = settings.overlap_percent
    
    tiles = list(generate_overlapping_tiles(test_shape, chunk_size, overlap))
    print(f"Image shape: {test_shape}")
    print(f"Chunk size: {chunk_size}x{chunk_size}")
    print(f"Overlap: {overlap}%")
    print(f"Total tiles: {len(tiles)}")
    print(f"Stride: {chunk_size - int(chunk_size * overlap / 100)} pixels")
    print()
    
    # Show first few tiles
    for i, tile in enumerate(tiles[:5]):
        print(f"Tile {tile['tile_id']}: rows [{tile['row_start']}:{tile['row_end']}] "
              f"cols [{tile['col_start']}:{tile['col_end']}] "
              f"({tile['width']}x{tile['height']}) "
              f"edge={tile['is_edge']}")
    
    print(f"\n... and {len(tiles) - 5} more tiles")
    
    # Calculate coverage
    total_pixels = sum(t['width'] * t['height'] for t in tiles)
    image_pixels = test_shape[0] * test_shape[1]
    print(f"\nTotal tile pixels: {total_pixels:,}")
    print(f"Image pixels: {image_pixels:,}")
    print(f"Overlap overhead: {100 * (total_pixels / image_pixels - 1):.1f}%")
