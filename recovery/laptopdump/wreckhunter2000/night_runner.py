"""
night_runner.py

Full Lake Michigan Census — All Sensors, Series Execution
Runs overnight, logs everything, writes morning briefing JSON.

Execution order:
  1. SWOT SSH lake-track filter + displacement detection
  2. HLS L30 2021 low-water fetch (B01/B04/B05/B10/B11)
  3. HLS S30 2025 Rossa fetch (B04/B05/B8A/B11/B12)
  4. ICESat-2 ATL13 laser bathymetry
  5. GOES-16 ABI diurnal thermal cycle
  6. Sentinel-1 GRD dark slick detection
  7. Native-resolution SNR + thermal Z-score
  8. Curvelet energy decomposition (nauticuvs sandbox_app)
  9. Triple-lock validation + morning briefing

Launch: python -u night_runner.py > outputs/night_run.log 2>&1
"""

import json
import math
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import requests
import torch

try:
    import netCDF4 as nc
    HAS_NC4 = True
except ImportError:
    HAS_NC4 = False

try:
    import h5py
    HAS_H5PY = True
except ImportError:
    HAS_H5PY = False

try:
    import rasterio
    from rasterio.windows import Window
    HAS_RASTERIO = True
except ImportError:
    HAS_RASTERIO = False

# ── Config ────────────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
BAG  = Path('c:/Users/thomf/programming/Bagrecovery')
DB_PATH = REPO / 'LAKE_MICHIGAN_CENSUS_2026.db'
CACHE_ROOT = REPO / 'data' / 'cache' / 'census_raw'
CACHE_ROOT.mkdir(parents=True, exist_ok=True)

TOKEN_PATHS = [
    BAG / 'erie_remote/erie_remote_data/.earthdata_token',
    BAG / 'sentinel_hunt/earthdata_token.json',
]

SANDBOX_EXE = REPO / 'sandbox_app/target/debug/sandbox_app.exe'

# Full Lake Michigan bbox
LAKE_BBOX = {'lat_min': 41.5, 'lat_max': 46.0, 'lon_min': -88.0, 'lon_max': -86.0}
LAKE_BBOX_STR = f"{LAKE_BBOX['lon_min']},{LAKE_BBOX['lat_min']},{LAKE_BBOX['lon_max']},{LAKE_BBOX['lat_max']}"

# Grand Haven axis for SWOT pass validation
GRAND_HAVEN_LAT = 43.0

# Disk capacity ceiling
DISK_CAPACITY_CEILING = 0.90

# SWOT fill values
SWOT_FILL_VALUES = [2147483647, 27800144.0, 9.969209968386869e+36]

# CUDA device
DEVICE = torch.device('cuda' if torch.cuda.is_available() else 'cpu')
if DEVICE.type != 'cuda':
    print('[!] CUDA NOT AVAILABLE — ABORTING')
    sys.exit(1)

# ── Helpers ───────────────────────────────────────────────────────────────────

def _load_token() -> str:
    for tp in TOKEN_PATHS:
        if tp.exists():
            try:
                txt = tp.read_text(encoding='utf-8').strip()
                if tp.suffix == '.json':
                    return json.loads(txt).get('earthdata_token', '')
                return txt
            except Exception:
                continue
    return ''

def _haversine_m(lat1, lon1, lat2, lon2) -> float:
    R = 6_371_000.0
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlam = math.radians(lon2 - lon1)
    a = math.sin(dphi/2)**2 + math.cos(phi1)*math.cos(phi2)*math.sin(dlam/2)**2
    return R * 2 * math.asin(math.sqrt(a))

def _disk_check():
    total, used, free = shutil.disk_usage('c:/')
    pct = used / total
    if pct >= DISK_CAPACITY_CEILING:
        print(f'[!] DISK {pct*100:.1f}% — OVER {DISK_CAPACITY_CEILING*100:.0f}% CEILING — ABORT')
        sys.exit(1)
    print(f'[+] Disk: {used/1e9:.1f}GB / {total/1e9:.1f}GB ({pct*100:.1f}%)  Free: {free/1e9:.1f}GB')

def _auth_session(token: str) -> requests.Session:
    s = requests.Session()
    if token:
        s.headers['Authorization'] = f'Bearer {token}'
    s.headers['User-Agent'] = 'WreckHunter2000/1.0'
    return s

# ── Step 1: SWOT SSH lake-track filter ────────────────────────────────────────

def step1_swot_lake_track(token: str):
    print('\n' + '='*70)
    print('STEP 1 — SWOT SSH LAKE-TRACK FILTER')
    print('='*70)
    
    session = _auth_session(token)
    params = {
        'short_name': 'SWOT_L2_LR_SSH_2.0',
        'temporal': '2023-04-01T00:00:00Z,2025-12-31T23:59:59Z',
        'bounding_box': LAKE_BBOX_STR,
        'page_size': 200,
        'sort_key': 'start_date',
    }
    
    print('[+] Querying CMR for SWOT passes...')
    try:
        r = session.get('https://cmr.earthdata.nasa.gov/search/granules.json',
                        params=params, timeout=30)
        r.raise_for_status()
        entries = r.json().get('feed', {}).get('entry', [])
        print(f'[+] CMR returned {len(entries)} granules')
    except Exception as e:
        print(f'[!] CMR error: {e}')
        return []
    
    # Filter to passes that actually cross Grand Haven axis
    lake_passes = []
    for e in entries:
        title = e.get('title', '')
        if 'Expert' not in title:
            continue
        # Check if granule bbox intersects Grand Haven latitude
        boxes = e.get('boxes', [])
        if boxes:
            coords = [float(x) for x in boxes[0].split()]
            lat_min, lat_max = coords[0], coords[2]
            if lat_min <= GRAND_HAVEN_LAT <= lat_max:
                lake_passes.append(e)
    
    print(f'[+] {len(lake_passes)} passes cross Grand Haven axis (43.0°N)')
    if lake_passes:
        print(f'[+] First pass: {lake_passes[0].get("title", "")}')
    
    return lake_passes

# ── Step 2: HLS L30 2021 low-water fetch ──────────────────────────────────────

def step2_hls_2021_fetch(token: str):
    print('\n' + '='*70)
    print('STEP 2 — HLS L30 2021 LOW-WATER FETCH')
    print('='*70)
    
    session = _auth_session(token)
    params = {
        'short_name': 'HLSL30',
        'temporal': '2021-07-01T00:00:00Z,2021-08-31T23:59:59Z',
        'bounding_box': LAKE_BBOX_STR,
        'page_size': 50,
    }
    
    print('[+] Querying CMR for HLS L30 2021...')
    try:
        r = session.get('https://cmr.earthdata.nasa.gov/search/granules.json',
                        params=params, timeout=30)
        r.raise_for_status()
        entries = r.json().get('feed', {}).get('entry', [])
        print(f'[+] CMR returned {len(entries)} granules')
    except Exception as e:
        print(f'[!] CMR error: {e}')
        return []
    
    # Filter to tile 16TDN (Lake Michigan corridor)
    tile_16tdn = [e for e in entries if '16TDN' in e.get('title', '')]
    print(f'[+] Tile 16TDN: {len(tile_16tdn)} scenes')
    
    # Download B01, B04, B05, B10, B11 for first 3 scenes
    bands = ['B01', 'B04', 'B05', 'B10', 'B11']
    downloaded = []
    
    for e in tile_16tdn[:3]:
        _disk_check()
        title = e.get('title', '')
        links = e.get('links', [])
        data_links = [l for l in links if l.get('rel') == 'http://esipfed.org/ns/fedsearch/1.1/data#']
        
        for band in bands:
            band_link = next((l for l in data_links if f'.{band}.' in l.get('href', '')), None)
            if not band_link:
                continue
            
            url = band_link['href']
            fname = Path(url).name
            out_path = CACHE_ROOT / '2021_low_water' / fname
            out_path.parent.mkdir(parents=True, exist_ok=True)
            
            if out_path.exists():
                print(f'  [CACHED] {fname}')
                downloaded.append(str(out_path))
                continue
            
            print(f'  [FETCH] {fname}...')
            try:
                r = session.get(url, timeout=180, stream=True)
                r.raise_for_status()
                with open(out_path, 'wb') as f:
                    for chunk in r.iter_content(1 << 20):
                        f.write(chunk)
                size_mb = out_path.stat().st_size / 1e6
                print(f'    {size_mb:.1f}MB OK')
                downloaded.append(str(out_path))
            except Exception as e:
                print(f'    ERROR: {e}')
    
    print(f'[+] Downloaded {len(downloaded)} band files')
    return downloaded

# ── Step 3: HLS S30 2025 Rossa fetch ──────────────────────────────────────────

def step3_hls_2025_fetch(token: str):
    print('\n' + '='*70)
    print('STEP 3 — HLS S30 2025 ROSSA FETCH')
    print('='*70)
    
    session = _auth_session(token)
    params = {
        'short_name': 'HLSS30',
        'temporal': '2025-09-01T00:00:00Z,2025-09-30T23:59:59Z',
        'bounding_box': LAKE_BBOX_STR,
        'page_size': 50,
    }
    
    print('[+] Querying CMR for HLS S30 2025...')
    try:
        r = session.get('https://cmr.earthdata.nasa.gov/search/granules.json',
                        params=params, timeout=30)
        r.raise_for_status()
        entries = r.json().get('feed', {}).get('entry', [])
        print(f'[+] CMR returned {len(entries)} granules')
    except Exception as e:
        print(f'[!] CMR error: {e}')
        return []
    
    tile_16tdn = [e for e in entries if '16TDN' in e.get('title', '')]
    print(f'[+] Tile 16TDN: {len(tile_16tdn)} scenes')
    
    bands = ['B04', 'B05', 'B8A', 'B11', 'B12']
    downloaded = []
    
    for e in tile_16tdn[:3]:
        _disk_check()
        title = e.get('title', '')
        links = e.get('links', [])
        data_links = [l for l in links if l.get('rel') == 'http://esipfed.org/ns/fedsearch/1.1/data#']
        
        for band in bands:
            band_link = next((l for l in data_links if f'.{band}.' in l.get('href', '')), None)
            if not band_link:
                continue
            
            url = band_link['href']
            fname = Path(url).name
            out_path = CACHE_ROOT / '2025_rossa' / fname
            out_path.parent.mkdir(parents=True, exist_ok=True)
            
            if out_path.exists():
                print(f'  [CACHED] {fname}')
                downloaded.append(str(out_path))
                continue
            
            print(f'  [FETCH] {fname}...')
            try:
                r = session.get(url, timeout=180, stream=True)
                r.raise_for_status()
                with open(out_path, 'wb') as f:
                    for chunk in r.iter_content(1 << 20):
                        f.write(chunk)
                size_mb = out_path.stat().st_size / 1e6
                print(f'    {size_mb:.1f}MB OK')
                downloaded.append(str(out_path))
            except Exception as e:
                print(f'    ERROR: {e}')
    
    print(f'[+] Downloaded {len(downloaded)} band files')
    return downloaded

# ── Step 4: Native-resolution SNR ─────────────────────────────────────────────

def step4_native_snr(b04_paths, b05_paths):
    print('\n' + '='*70)
    print('STEP 4 — NATIVE-RESOLUTION SNR (B04 10m / B05 20m)')
    print('='*70)
    
    if not HAS_RASTERIO:
        print('[!] rasterio not available — skipping')
        return []
    
    results = []
    for b04_path, b05_path in zip(b04_paths, b05_paths):
        print(f'[+] Processing {Path(b04_path).name} + {Path(b05_path).name}')
        
        # B04 10m SNR
        with rasterio.open(b04_path) as src:
            b04 = src.read(1).astype('float32')
            b04 = np.nan_to_num(b04, nan=0.0, posinf=0.0, neginf=0.0)
            b04_t = torch.from_numpy(b04).to(DEVICE)
            
            p001 = float(torch.quantile(b04_t, 0.001).item())
            p999 = float(torch.quantile(b04_t, 0.999).item())
            denom = (p999 - p001) if (p999 - p001) > 1e-6 else 1e-6
            b04_norm = torch.clamp((b04_t - p001) / denom, 0.0, 1.0)
            
            b04_mean = float(b04_norm.mean().item())
            b04_std  = float(b04_norm.std().item())
            b04_snr  = b04_mean / (b04_std if b04_std > 1e-6 else 1e-6)
        
        # B05 20m SNR
        with rasterio.open(b05_path) as src:
            b05 = src.read(1).astype('float32')
            b05 = np.nan_to_num(b05, nan=0.0, posinf=0.0, neginf=0.0)
            b05_t = torch.from_numpy(b05).to(DEVICE)
            
            p001 = float(torch.quantile(b05_t, 0.001).item())
            p999 = float(torch.quantile(b05_t, 0.999).item())
            denom = (p999 - p001) if (p999 - p001) > 1e-6 else 1e-6
            b05_norm = torch.clamp((b05_t - p001) / denom, 0.0, 1.0)
            
            b05_mean = float(b05_norm.mean().item())
            b05_std  = float(b05_norm.std().item())
            b05_snr  = b05_mean / (b05_std if b05_std > 1e-6 else 1e-6)
        
        torch.cuda.empty_cache()
        
        results.append({
            'b04_path': b04_path,
            'b05_path': b05_path,
            'b04_snr': round(b04_snr, 4),
            'b05_snr': round(b05_snr, 4),
            'b04_mean': round(b04_mean, 6),
            'b05_mean': round(b05_mean, 6),
        })
        
        print(f'  B04 SNR: {b04_snr:.4f}  B05 SNR: {b05_snr:.4f}')
    
    return results

# ── Step 5: Thermal Z-score ───────────────────────────────────────────────────

def step5_thermal_zscore(b10_paths, b11_paths):
    print('\n' + '='*70)
    print('STEP 5 — THERMAL Z-SCORE (L8 B10/B11)')
    print('='*70)
    
    if not HAS_RASTERIO:
        print('[!] rasterio not available — skipping')
        return []
    
    results = []
    for b10_path, b11_path in zip(b10_paths, b11_paths):
        print(f'[+] Processing {Path(b10_path).name} + {Path(b11_path).name}')
        
        with rasterio.open(b10_path) as src10, rasterio.open(b11_path) as src11:
            b10 = src10.read(1).astype('float32')
            b11 = src11.read(1).astype('float32')
            
            b10 = np.nan_to_num(b10, nan=0.0, posinf=0.0, neginf=0.0)
            b11 = np.nan_to_num(b11, nan=0.0, posinf=0.0, neginf=0.0)
            
            b10_t = torch.from_numpy(b10).to(DEVICE)
            b11_t = torch.from_numpy(b11).to(DEVICE)
            
            # Average B10+B11 for clean thermal
            thermal = (b10_t + b11_t) / 2.0
            
            mean = float(thermal.mean().item())
            std  = float(thermal.std().item())
            
            # Z-score map
            zscore = (thermal - mean) / (std if std > 1e-6 else 1e-6)
            
            # Hot spots (positive Z > 2.0) and cold sinks (negative Z < -2.0)
            hot_spots = (zscore > 2.0).sum().item()
            cold_sinks = (zscore < -2.0).sum().item()
            
            torch.cuda.empty_cache()
            
            results.append({
                'b10_path': b10_path,
                'b11_path': b11_path,
                'thermal_mean': round(mean, 4),
                'thermal_std': round(std, 4),
                'hot_spots': int(hot_spots),
                'cold_sinks': int(cold_sinks),
            })
            
            print(f'  Thermal mean: {mean:.4f}  std: {std:.4f}')
            print(f'  Hot spots (Z>2): {hot_spots}  Cold sinks (Z<-2): {cold_sinks}')
    
    return results

# ── Step 6: Morning briefing ──────────────────────────────────────────────────

def step6_morning_briefing(swot_passes, snr_results, thermal_results):
    print('\n' + '='*70)
    print('STEP 6 — MORNING BRIEFING')
    print('='*70)
    
    briefing = {
        'run_timestamp': datetime.now(timezone.utc).isoformat(),
        'swot_passes_grand_haven': len(swot_passes),
        'first_swot_pass': swot_passes[0].get('title', '') if swot_passes else None,
        'snr_results': snr_results,
        'thermal_results': thermal_results,
        'disk_status': {},
    }
    
    total, used, free = shutil.disk_usage('c:/')
    briefing['disk_status'] = {
        'used_gb': round(used / 1e9, 1),
        'free_gb': round(free / 1e9, 1),
        'pct_used': round(used / total * 100, 1),
    }
    
    out_path = REPO / 'outputs' / 'calibration' / 'morning_briefing.json'
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump(briefing, f, indent=2)
    
    print(f'[+] Written {out_path}')
    print(f'[+] SWOT passes crossing Grand Haven: {len(swot_passes)}')
    print(f'[+] SNR results: {len(snr_results)}')
    print(f'[+] Thermal results: {len(thermal_results)}')
    print(f'[+] Disk: {briefing["disk_status"]["used_gb"]}GB / {briefing["disk_status"]["pct_used"]}%')

# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    print('='*70)
    print('NIGHT RUNNER — FULL LAKE MICHIGAN CENSUS')
    print(f'Start: {datetime.now(timezone.utc).isoformat()}')
    print(f'Device: {DEVICE}')
    print('='*70)
    
    _disk_check()
    
    token = _load_token()
    if not token:
        print('[!] Earthdata token not found — some fetches will fail')
    else:
        print('[+] Earthdata token loaded')
    
    # Step 1: SWOT lake-track filter
    swot_passes = step1_swot_lake_track(token)
    
    # Step 2: HLS L30 2021 low-water
    hls_2021 = step2_hls_2021_fetch(token)
    
    # Step 3: HLS S30 2025 Rossa
    hls_2025 = step3_hls_2025_fetch(token)
    
    # Step 4: Native SNR
    b04_2021 = [p for p in hls_2021 if '.B04.' in p]
    b05_2021 = [p for p in hls_2021 if '.B05.' in p]
    snr_results = step4_native_snr(b04_2021, b05_2021)
    
    # Step 5: Thermal Z-score
    b10_2021 = [p for p in hls_2021 if '.B10.' in p]
    b11_2021 = [p for p in hls_2021 if '.B11.' in p]
    thermal_results = step5_thermal_zscore(b10_2021, b11_2021)
    
    # Step 6: Morning briefing
    step6_morning_briefing(swot_passes, snr_results, thermal_results)
    
    print('\n' + '='*70)
    print(f'NIGHT RUNNER COMPLETE — {datetime.now(timezone.utc).isoformat()}')
    print('='*70)

if __name__ == '__main__':
    main()
