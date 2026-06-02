import argparse
import json
import os
import sys
from pathlib import Path

import numpy as np
import torch
import rasterio

# Enable local imports if this script is run directly
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from data_fetcher_scavenger import ensure_tiles
from wreckhunter_calibrator import WreckHunterCalibrator

CACHE_DIR = Path(__file__).resolve().parents[2] / 'Bagrecovery' / 'outputs' / 'rossa_forensic_cache'

ROSSA_WINDOW = 'rossa'
BASELINE_WINDOW = 'baseline'


def assert_cuda():
    if not torch.cuda.is_available():
        raise RuntimeError('CUDA is unavailable. Install drivers and ensure M2200 visibility.')
    print('[+] CUDA device:', torch.cuda.get_device_name(0))


def bfile(tile, band):
    # L30 preferred for B01/B03/B04 (Landsat native 30m), S30 for B08 (Sentinel only)
    # Accept whichever product is already on disk
    for prefix in ('HLS.L30', 'HLS.S30'):
        p = CACHE_DIR / f'{prefix}.{tile}.{band}.tif'
        if p.exists() and p.stat().st_size > 0:
            return p
    # not cached yet — return S30 path so require_file triggers fetch
    return CACHE_DIR / f'HLS.S30.{tile}.{band}.tif'


def require_file(tile, band, window):
    path = bfile(tile, band)
    if path.exists() and path.stat().st_size > 0:
        print(f'[+] Found existing {path} ({path.stat().st_size} bytes)')
        return path

    print(f'[!] Missing {path}, fetching via data_fetcher_scavenger...')
    from data_fetcher_scavenger import download_band
    path = download_band(tile, band, window)
    print(f'[+] Downloaded {path} ({path.stat().st_size} bytes)')
    return path


def load_band(path):
    """Load a band at its native resolution. Never resample raw DN values."""
    with rasterio.open(path) as src:
        arr = src.read(1).astype(np.float32)
        if arr.max() > 10:
            arr = arr * 0.0001
    return arr


def upsample_result(result_map: np.ndarray, target_shape: tuple) -> np.ndarray:
    """
    Upsample a derived result map (anomaly scores, z-scores, ratios) to
    target_shape using bilinear interpolation.
    Only call this on computed outputs — never on raw band data.
    """
    if result_map.shape == target_shape:
        return result_map
    import rasterio.transform
    from rasterio.enums import Resampling
    from rasterio.io import MemoryFile
    # write to in-memory raster then read back at target shape
    with MemoryFile() as mem:
        with mem.open(
            driver='GTiff', height=result_map.shape[0], width=result_map.shape[1],
            count=1, dtype='float32',
        ) as ds:
            ds.write(result_map[np.newaxis, ...])
            out = ds.read(
                1,
                out_shape=(1, target_shape[0], target_shape[1]),
                resampling=Resampling.bilinear,
            )
    return out


def _native_shape(path) -> tuple:
    with rasterio.open(path) as src:
        return (src.height, src.width)


def run_glint_scan(tile):
    b08_path = require_file(tile, 'B08', ROSSA_WINDOW)
    b04_path = require_file(tile, 'B04', ROSSA_WINDOW)  # 10m like B08, no resampling needed
    b01_path = require_file(tile, 'B01', ROSSA_WINDOW)  # 60m — parsed at native, result upsampled

    # Each band parsed at its own native resolution
    b08 = load_band(b08_path)   # 10m  10980x10980
    b04 = load_band(b04_path)   # 10m  10980x10980
    b01 = load_band(b01_path)   # 60m  1830x1830

    # ── Compute metrics at native resolution per band ──────────────────────
    # Glint ratio using same-resolution bands (B08/B04 both 10m)
    glint_10m = b08 / (b04 + 1e-6)
    mean_glint = float(np.nanmean(glint_10m))
    std_glint  = float(np.nanstd(glint_10m))
    glint_mask_10m = glint_10m > mean_glint + 2 * std_glint

    # B01 atmospheric score at native 60m — scalar stats only, no pixel math with other bands
    b01_mean = float(np.nanmean(b01))
    b01_std  = float(np.nanstd(b01))
    # Upsample the B01 anomaly score map to 10m for spatial overlay logging
    b01_zscore_60m = (b01 - b01_mean) / (b01_std + 1e-9)
    b01_zscore_10m = upsample_result(b01_zscore_60m, glint_10m.shape)

    result = {
        'tile': tile,
        'b08_path': str(b08_path),
        'b04_path': str(b04_path),
        'b01_path': str(b01_path),
        'glint_mean': mean_glint,
        'glint_std': std_glint,
        'glint_hotspots': int(np.sum(glint_mask_10m)),
        'b01_atm_mean': b01_mean,
        'b01_atm_std': b01_std,
        # hotspots where both glint AND atmospheric loading are elevated
        'combined_hotspots': int(np.sum(glint_mask_10m & (b01_zscore_10m > 1.0))),
    }
    print(f"[+] {tile} glint mean={mean_glint:.6f} std={std_glint:.6f} "
          f"hotspots={result['glint_hotspots']} combined={result['combined_hotspots']}")

    # Thermal sink: baseline B08 at native 10m, z-score vs rossa
    b08_base_path = require_file(tile, 'B08', BASELINE_WINDOW)
    b08_base = load_band(b08_base_path)  # native 10m
    z = (glint_10m - float(np.nanmean(b08_base))) / (float(np.nanstd(b08_base)) + 1e-9)
    result['thermal_sink_points'] = int(np.sum(z < -1.6))
    print(f"[+] {tile} thermal sink points (z<-1.6) = {result['thermal_sink_points']}")

    return result


def compute_real_michigan_snr():
    tile = 'T16TET'
    b04_path = require_file(tile, 'B04', ROSSA_WINDOW)  # 10m
    b05_path = require_file(tile, 'B05', ROSSA_WINDOW)  # 20m

    calibrator = WreckHunterCalibrator()
    result = calibrator.dual_stream_native_snr(str(b04_path), str(b05_path))

    print(f'[+] Michigan B04 (10m) SNR = {result["b04_snr"]:.4f}')
    print(f'[+] Michigan B05 (20m) SNR = {result["b05_snr"]:.4f}')
    print(f'[+] Turbidity proxy (B04_20m/B05_20m) = {result["turbidity_proxy"]:.4f}')
    print(f'[+] Rossa signatures: {result["rossa_patches"]} patches, '
          f'{result["rossa_total_px"]} pixels')

    turbidity = result['turbidity_proxy']
    snr       = result['b04_snr']  # primary SNR is the 10m band

    out = {
        'timestamp':        Path(__file__).stat().st_mtime,
        'tile':             tile,
        'b04_snr_10m':      result['b04_snr'],
        'b05_snr_20m':      result['b05_snr'],
        'turbidity_proxy':  result['turbidity_proxy'],
        'rossa_patches':    result['rossa_patches'],
        'rossa_total_px':   result['rossa_total_px'],
        'rossa_detections': result['rossa_detections'],
        'b04_origin':       result['b04_origin'],
        'b05_origin':       result['b05_origin'],
        'noise_floor':      1.729,
        'baked_noise_floor': snr,
    }
    out_path = Path(__file__).resolve().parents[2] / 'outputs' / 'calibration' / 'system_limits_v1.json'
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump(out, f, indent=2)
    print(f'[+] Written {out_path}')
    return snr
    print(f"[+] Michigan real SNR = {snr:.6f}, turbidity={turbidity:.6f}")

    # update system_limits_v1.json
    out = {
        'timestamp': Path(__file__).stat().st_mtime,
        'michigan_snr': snr,
        'michigan_wind_kts': 12.1,
        'noise_floor': 1.729,
        'baked_noise_floor': snr,
    }
    out_path = Path(__file__).resolve().parents[2] / 'dist' / 'WreckHunter2000_Clean' / 'system_limits_v1.json'
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump(out, f, indent=2)
    print(f"[+] Updated {out_path}")
    return snr


def main():
    parser = argparse.ArgumentParser(description='Day Zero strict calibration')
    parser.add_argument('--lake', required=True, choices=['Michigan', 'Huron', 'Erie'])
    parser.add_argument('--bbox-west', type=float, required=True)
    parser.add_argument('--bbox-south', type=float, required=True)
    parser.add_argument('--bbox-east', type=float, required=True)
    parser.add_argument('--bbox-north', type=float, required=True)

    args = parser.parse_args()

    assert_cuda()

    # Hard no-sim fallback: fetch or abort.
    tile_map = {'Michigan': 'T16TET', 'Huron': 'T17TLC', 'Erie': 'T17TNE'}
    tile = tile_map[args.lake]

    print(f"[+] Running strict Day Zero for {args.lake} with bbox {(args.bbox_west, args.bbox_south, args.bbox_east, args.bbox_north)}")

    # load B01/B08 from rossa window for glint scan
    glint_info = run_glint_scan(tile)

    # run SNR calibration for Michigan only, as requested
    mich_snr = None
    if args.lake == 'Michigan':
        mich_snr = compute_real_michigan_snr()

    print('[+] CAST: glint_info=', glint_info)
    print('[+] CUDA device used:', torch.cuda.get_device_name(0))
    if mich_snr is not None:
        print('[+] Michigan SNR (real):', mich_snr)


if __name__ == '__main__':
    main()
