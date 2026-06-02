"""
run_band_scans.py — Push all on-disk HLS .tif bands for Erie, Michigan, and Huron
to the GPU one at a time, compute stretch stats, and patch the results into
calibration_v1.json under each site's 'band_scans' key.

Skips any band already present in band_scans so re-runs are safe.
Thermal gate: nvidia-smi checked at most once every 120 s; 3 s warn / 4 s hard loop.
"""

import io
import json
import subprocess
import sys
import time
import warnings
from pathlib import Path

# Force UTF-8 output on Windows cp1252 consoles
if sys.stdout.encoding and sys.stdout.encoding.lower() != 'utf-8':
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8', errors='replace')

import numpy as np

try:
    import torch
    HAS_TORCH = torch.cuda.is_available()
except ImportError:
    HAS_TORCH = False

try:
    import rasterio
    HAS_RASTERIO = True
except ImportError:
    HAS_RASTERIO = False

# ── Paths ──────────────────────────────────────────────────────────────────────

CAL_DIR  = Path(__file__).parent.parent / 'outputs' / 'calibration'
CAL_FILE = CAL_DIR / 'calibration_v1.json'

SITE_DIRS = {
    'lake_erie':     CAL_DIR / 'lake_erie',
    'lake_michigan': CAL_DIR / 'lake_michigan',
    'lake_huron':    CAL_DIR / 'lake_huron',
}

# ── Thermal gate ───────────────────────────────────────────────────────────────

_THERMAL_INTERVAL = 120.0
_last_check: float = 0.0
GPU_WARN  = 78
GPU_LIMIT = 83


def _gpu_temp() -> int | None:
    try:
        out = subprocess.check_output(
            ['nvidia-smi', '--query-gpu=temperature.gpu', '--format=csv,noheader,nounits'],
            timeout=5, stderr=subprocess.DEVNULL,
        )
        return int(out.decode().strip().splitlines()[0])
    except Exception:
        return None


def _thermal_gate(label: str = '') -> None:
    global _last_check
    if not HAS_TORCH:
        return
    now = time.monotonic()
    if now - _last_check < _THERMAL_INTERVAL:
        return
    _last_check = now
    torch.cuda.empty_cache()
    temp = _gpu_temp()
    if temp is None:
        return
    if temp >= GPU_LIMIT:
        warnings.warn(f'GPU {temp}°C — hard pause {label}')
        while True:
            time.sleep(4)
            temp = _gpu_temp()
            if temp is None or temp < GPU_WARN:
                print(f'  GPU cooled to {temp}°C — resuming')
                break
    elif temp >= GPU_WARN:
        warnings.warn(f'GPU {temp}°C — brief break {label}')
        time.sleep(3)


# ── Per-band squeeze ───────────────────────────────────────────────────────────

def squeeze_band(path: Path) -> dict:
    """Load full band to GPU, return stretch stats. Falls back to numpy on OOM."""
    with rasterio.open(path) as src:
        arr = src.read(1).astype(np.float32)
    arr = np.nan_to_num(arr, nan=0.0, posinf=0.0, neginf=0.0)
    if arr.max() > 10:
        arr *= 0.0001

    if HAS_TORCH:
        try:
            t = torch.from_numpy(arr).cuda()
            p_low  = float(torch.quantile(t, 0.001))
            p_high = float(torch.quantile(t, 0.999))
            mean   = float(t.mean())
            std    = float(t.std())
            del t
            torch.cuda.empty_cache()
            return {'stretch_min': p_low, 'stretch_max': p_high,
                    'mean': mean, 'std': std, 'source': 'cuda'}
        except Exception:
            torch.cuda.empty_cache()

    # numpy fallback
    flat = arr.flatten()
    return {
        'stretch_min': float(np.percentile(flat, 0.1)),
        'stretch_max': float(np.percentile(flat, 99.9)),
        'mean':        float(np.mean(flat)),
        'std':         float(np.std(flat)),
        'source':      'numpy',
    }


# ── Main ───────────────────────────────────────────────────────────────────────

def main():
    if not HAS_RASTERIO:
        raise RuntimeError('rasterio required')

    cal = json.loads(CAL_FILE.read_text(encoding='utf-8'))

    # Index existing site entries by site_id for easy lookup
    sites_by_id = {s['site_id']: s for s in cal['sites']}

    device = torch.cuda.get_device_name(0) if HAS_TORCH else 'cpu'
    print(f'[+] Device: {device}')

    for site_id, site_dir in SITE_DIRS.items():
        tifs = sorted(site_dir.glob('*.tif'))
        print(f'\n[+] {site_id} — {len(tifs)} files')

        site = sites_by_id.setdefault(site_id, {'site_id': site_id, 'band_scans': {}})
        band_scans = site.setdefault('band_scans', {})

        for tif in tifs:
            band = tif.stem.split('.')[-1]  # B01, Fmask, SAA …
            if band in band_scans:
                print(f'  skip {band} (already done)')
                continue
            try:
                stats = squeeze_band(tif)
                band_scans[band] = stats
                print(f'  ✅ {band}: min={stats["stretch_min"]:.4f} '
                      f'max={stats["stretch_max"]:.4f} mean={stats["mean"]:.4f} '
                      f'[{stats["source"]}]')
            except Exception as e:
                band_scans[band] = {'error': str(e)}
                print(f'  ❌ {band}: {e}')

            _thermal_gate(f'{site_id}/{band}')

        # Write after each site so a crash mid-run doesn't lose work
        CAL_FILE.write_text(json.dumps(cal, indent=2), encoding='utf-8')
        print(f'  💾 calibration_v1.json updated ({site_id})')

    print('\n✅ All band scans complete.')


if __name__ == '__main__':
    main()
