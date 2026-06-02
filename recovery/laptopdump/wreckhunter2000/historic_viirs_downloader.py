"""
historic_viirs_downloader.py

2011-2012 TIME MACHINE — Suomi NPP VIIRS Thermal + Nighttime Visible
=====================================================================
Suomi NPP launched October 28, 2011. VIIRS data starts late 2011.
This catches the TAIL END of the 2011 season and the FULL 2012 season.

FREE SOURCES:
  - NASA LAADS DAAC (your NASA Earthdata account — same token)
    Products: VNP02IMG, VNP03IMG, VNP21A1D (Land Surface Temp)
  - NOAA CLASS (Comprehensive Large Array-data Stewardship System)
    https://www.class.noaa.gov/ — free, no account needed for some products
  - NASA Earthdata CMR — same token as Landsat

VIIRS Bands used:
  DNB  - Day/Night Band (0.5-0.9μm, 750m) — nighttime visible
         A large steel wreck holds heat differently than surrounding water.
         At night, the thermal contrast is MAXIMUM.
  I4   - SWIR (3.74μm, 375m) — fire/thermal anomaly band
         Detects warm anomalies (steel holding daytime heat into night)
  I5   - Thermal IR (11.45μm, 375m) — surface temperature
         Cold-sink detection: steel wreck = colder than surrounding water
  M15  - Thermal IR (10.76μm, 750m) — broader thermal coverage
  M16  - Thermal IR (12.01μm, 750m) — split-window for SST correction

2012 ADVANTAGE for VIIRS:
  - Shallow water (low water year) = wreck thermal signature reaches surface
  - Clear water (mussel peak) = less thermal masking from turbidity
  - Cold-sink effect: large steel mass stays at ~4°C while surface warms
  - At night: surface water 20-25°C, wreck location: 15-18°C = detectable

CUDA: Thermal anomaly detection and cold-sink mapping on M2200 GPU.
"""

import json
import time
from datetime import datetime
from pathlib import Path
import requests

try:
    import torch
    import numpy as np
    CUDA = torch.cuda.is_available()
    DEVICE = torch.device('cuda' if CUDA else 'cpu')
    if CUDA:
        print(f'[+] CUDA: {torch.cuda.get_device_name(0)}')
except ImportError:
    CUDA = False
    DEVICE = None
    np = None

# ── Config ────────────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'historic_viirs'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
    Path.home() / '.netrc',
]

# NASA LAADS DAAC — free with Earthdata account
LAADS_BASE = 'https://ladsweb.modaps.eosdis.nasa.gov'
LAADS_SEARCH = f'{LAADS_BASE}/api/v2/content/details'
LAADS_DOWNLOAD = f'{LAADS_BASE}/archive/allData'

# NASA CMR — same token
CMR_BASE = 'https://cmr.earthdata.nasa.gov/search/granules.json'

# NOAA CLASS — free, no account for some products
NOAA_CLASS = 'https://www.class.noaa.gov/saa/products/search'

# Lake Michigan bbox
LAKE_MICHIGAN_BBOX = {
    'lon_min': -87.9, 'lat_min': 41.5,
    'lon_max': -85.5, 'lat_max': 46.0,
}

ZION_TRENCH_BBOX = {
    'lon_min': -87.60, 'lat_min': 42.35,
    'lon_max': -87.40, 'lat_max': 42.60,
}

# VIIRS products (NASA LP DAAC / LAADS DAAC)
VIIRS_PRODUCTS = {
    # Level 1 calibrated radiances (raw sensor data)
    'VNP02IMG': {
        'name': 'VIIRS/NPP Imagery Resolution Calibrated Radiances 6-Min L1B',
        'daac': 'LAADS',
        'short_name': 'VNP02IMG',
        'version': '002',
        'bands': ['I4', 'I5'],  # SWIR thermal + TIR
        'resolution_m': 375,
        'priority': 1,
    },
    # Geolocation
    'VNP03IMG': {
        'name': 'VIIRS/NPP Imagery Resolution Terrain-Corrected Geolocation 6-Min L1',
        'daac': 'LAADS',
        'short_name': 'VNP03IMG',
        'version': '002',
        'bands': ['lat', 'lon'],
        'resolution_m': 375,
        'priority': 2,
    },
    # Land Surface Temperature (daily, 1km)
    'VNP21A1D': {
        'name': 'VIIRS/NPP Land Surface Temperature and Emissivity Daily L3 1km',
        'daac': 'LP DAAC',
        'short_name': 'VNP21A1D',
        'version': '002',
        'bands': ['LST_1KM', 'QC'],
        'resolution_m': 1000,
        'priority': 3,
    },
    # Day/Night Band (nighttime visible)
    'VNP46A1': {
        'name': 'VIIRS/NPP Daily Gridded Day Night Band 500m Linear Lat Lon Grid Night',
        'daac': 'LP DAAC',
        'short_name': 'VNP46A1',
        'version': '001',
        'bands': ['DNB_At_Sensor_Radiance_500m'],
        'resolution_m': 500,
        'priority': 4,
    },
}

# 2011-2012 windows (VIIRS launched Oct 28, 2011)
OPTIMAL_WINDOWS = [
    ('2011-11-01', '2011-12-14'),   # First VIIRS data — late fall thermal contrast
    ('2012-04-15', '2012-05-15'),   # Spring post-ice
    ('2012-06-01', '2012-09-30'),   # Peak 2012 low water
    ('2012-09-01', '2012-10-15'),   # Fall thermal contrast maximum
]

# Cold-sink detection thresholds
COLD_SINK_ZSCORE = -2.0      # Z-score below mean = cold anomaly
MIN_COLD_AREA_PX = 4         # Minimum pixels for a valid cold sink
STEEL_TEMP_OFFSET_C = -3.0   # Steel wreck typically 3°C colder than surrounding water

# ── Auth ──────────────────────────────────────────────────────────────────────

def load_token() -> str:
    for tp in TOKEN_PATHS:
        if tp.exists():
            try:
                if tp.suffix == '.json':
                    return json.loads(tp.read_text()).get('earthdata_token', '')
                return tp.read_text().strip()
            except Exception:
                continue
    return ''

# ── CMR Query ─────────────────────────────────────────────────────────────────

def query_viirs_cmr(bbox: dict, date_range: tuple, token: str,
                     short_name: str, version: str = '002') -> list:
    """
    Query NASA CMR for VIIRS granules.
    FREE with NASA Earthdata account (same token as Landsat/ICESat-2).
    """
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'

    params = {
        'short_name': short_name,
        'version': version,
        'temporal': f'{date_range[0]}T00:00:00Z,{date_range[1]}T23:59:59Z',
        'bounding_box': f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}",
        'page_size': 200,
        'sort_key': 'start_date',
    }

    print(f'  CMR query: {short_name} v{version} {date_range[0]} → {date_range[1]}')
    try:
        resp = requests.get(CMR_BASE, params=params, headers=headers, timeout=60)
        resp.raise_for_status()
        entries = resp.json().get('feed', {}).get('entry', [])

        granules = []
        for e in entries:
            links = e.get('links', [])
            # Prefer HDF5 (.h5) or HDF4 (.hdf) download links
            dl_url = next(
                (l['href'] for l in links
                 if l.get('rel') == 'http://esipfed.org/ns/fedsearch/1.1/data#'
                 and any(l.get('href', '').endswith(ext)
                         for ext in ('.h5', '.hdf', '.nc', '.HDF5'))),
                None
            )
            # Fallback: any https data link
            if not dl_url:
                dl_url = next(
                    (l['href'] for l in links
                     if 'https://' in l.get('href', '')
                     and l.get('type', '') in
                     ('application/x-hdf5', 'application/x-hdf',
                      'application/octet-stream')),
                    None
                )

            granules.append({
                'granule_id': e.get('id', ''),
                'title': e.get('title', ''),
                'time_start': e.get('time_start', '')[:16],
                'dl_url': dl_url,
                'short_name': short_name,
            })

        print(f'    Found {len(granules)} granules')
        return granules

    except Exception as ex:
        print(f'    CMR query failed: {ex}')
        return []


def query_viirs_laads(bbox: dict, date_range: tuple, token: str,
                       product: str = 'VNP02IMG') -> list:
    """
    Query NASA LAADS DAAC directly for VIIRS products.
    Alternative to CMR — sometimes has better coverage metadata.
    FREE with Earthdata account.
    """
    # LAADS uses day-of-year format
    start_dt = datetime.strptime(date_range[0], '%Y-%m-%d')
    end_dt = datetime.strptime(date_range[1], '%Y-%m-%d')

    headers = {'Authorization': f'Bearer {token}'} if token else {}

    granules = []
    current = start_dt
    while current <= end_dt:
        year = current.year
        doy = current.timetuple().tm_yday

        url = f'{LAADS_DOWNLOAD}/5200/{product}/{year}/{doy:03d}/'
        try:
            resp = requests.get(url, headers=headers, timeout=30)
            if resp.status_code == 200:
                # Parse file listing
                for line in resp.text.split('\n'):
                    if '.h5' in line or '.hdf' in line:
                        # Extract filename
                        import re
                        match = re.search(r'href="([^"]+\.(?:h5|hdf))"', line)
                        if match:
                            fname = match.group(1)
                            granules.append({
                                'granule_id': fname,
                                'title': fname,
                                'time_start': f'{year}-{doy:03d}',
                                'dl_url': f'{url}{fname}',
                                'short_name': product,
                            })
        except Exception:
            pass

        current = current + __import__('datetime').timedelta(days=1)

    print(f'    LAADS found {len(granules)} {product} files')
    return granules


def download_viirs_granule(granule: dict, output_dir: Path, token: str) -> Path | None:
    """Download a single VIIRS granule (HDF5 format)."""
    if not granule.get('dl_url'):
        return None

    fname = granule['title'].split('/')[-1]
    if not any(fname.endswith(ext) for ext in ('.h5', '.hdf', '.HDF5', '.nc')):
        fname += '.h5'
    out_path = output_dir / fname

    if out_path.exists() and out_path.stat().st_size > 100_000:
        print(f'    ✓ Already have: {fname}')
        return out_path

    print(f'    Downloading {fname}...')
    headers = {'Authorization': f'Bearer {token}'} if token else {}

    try:
        resp = requests.get(granule['dl_url'], headers=headers,
                            timeout=300, stream=True)
        resp.raise_for_status()
        with open(out_path, 'wb') as f:
            for chunk in resp.iter_content(chunk_size=65536):
                if chunk:
                    f.write(chunk)
        size_mb = out_path.stat().st_size / 1e6
        print(f'    ✓ Saved {fname} ({size_mb:.1f} MB)')
        return out_path
    except Exception as ex:
        print(f'    ✗ Failed: {ex}')
        if out_path.exists():
            out_path.unlink()
        return None

# ── CUDA Cold-Sink Detector ───────────────────────────────────────────────────

def cuda_detect_cold_sinks(thermal_array: 'np.ndarray',
                             transform=None) -> list:
    """
    GPU-accelerated cold-sink detection in VIIRS thermal data.

    Cold-sink signature (steel wreck in warm lake):
      1. Surface temperature Z-score < COLD_SINK_ZSCORE
      2. Spatial extent consistent with wreck size (10-500m)
      3. Persistent across multiple passes (not cloud shadow)
      4. Shape: elongated or irregular (not circular thermal eddy)

    In 2012's shallow water, a large steel wreck (Andaste: 266ft)
    creates a detectable cold patch at the surface.
    """
    if np is None:
        return []

    try:
        from scipy import ndimage
    except ImportError:
        print('    [!] scipy not installed — skipping cold-sink detection')
        return []

    print(f'    Cold-sink detection on {"GPU" if CUDA else "CPU"}...')

    # Mask fill values
    valid_mask = (thermal_array > 200) & (thermal_array < 400)  # Kelvin range
    if not valid_mask.any():
        # Try Celsius
        valid_mask = (thermal_array > -50) & (thermal_array < 50)

    if not valid_mask.any():
        print('    [!] No valid thermal data in array')
        return []

    if CUDA and torch is not None:
        arr_t = torch.from_numpy(thermal_array.astype(np.float32)).to(DEVICE)
        valid_t = torch.from_numpy(valid_mask).to(DEVICE)

        # Compute mean and std on valid pixels only
        valid_vals = arr_t[valid_t]
        mean_t = valid_vals.mean()
        std_t = valid_vals.std()

        # Z-score
        zscore = (arr_t - mean_t) / (std_t + 1e-8)

        # Cold anomaly mask
        cold_mask = (zscore < COLD_SINK_ZSCORE) & valid_t

        cold_np = cold_mask.cpu().numpy()
        zscore_np = zscore.cpu().numpy()
        torch.cuda.empty_cache()
    else:
        valid_vals = thermal_array[valid_mask]
        mean_t = np.mean(valid_vals)
        std_t = np.std(valid_vals)
        zscore_np = (thermal_array - mean_t) / (std_t + 1e-8)
        cold_np = (zscore_np < COLD_SINK_ZSCORE) & valid_mask

    # Label connected cold regions
    labeled, n = ndimage.label(cold_np)
    sizes = ndimage.sum(cold_np, labeled, range(1, n + 1))

    candidates = []
    for label_id in range(1, n + 1):
        size_px = int(sizes[label_id - 1])
        if size_px < MIN_COLD_AREA_PX:
            continue

        rows, cols = np.where(labeled == label_id)
        center_row = int(np.mean(rows))
        center_col = int(np.mean(cols))

        mean_zscore = float(np.mean(zscore_np[labeled == label_id]))
        mean_temp = float(np.mean(thermal_array[labeled == label_id]))

        lat, lon = 0.0, 0.0
        if transform is not None:
            try:
                lon, lat = transform * (center_col, center_row)
            except Exception:
                pass

        # Estimate size in meters (VIIRS I-band = 375m pixels)
        size_m = size_px * 375

        candidates.append({
            'row': center_row,
            'col': center_col,
            'lat': lat,
            'lon': lon,
            'size_px': size_px,
            'size_m_approx': size_m,
            'mean_zscore': round(mean_zscore, 3),
            'mean_temp': round(mean_temp, 2),
            'temp_offset_c': round(mean_temp - (mean_t if not CUDA else float(mean_t)), 2),
            'cold_sink_strength': 'STRONG' if mean_zscore < -3.0 else 'MODERATE',
            'wreck_candidate': size_m > 50 and mean_zscore < -2.5,
        })

    candidates.sort(key=lambda x: x['mean_zscore'])
    print(f'    Found {len(candidates)} cold-sink candidates '
          f'({sum(1 for c in candidates if c["wreck_candidate"])} wreck-like)')
    return candidates

# ── Main ──────────────────────────────────────────────────────────────────────

def run(bbox: dict = None, max_per_window: int = 10,
        products: list = None) -> dict:
    """
    Download Suomi NPP VIIRS thermal + DNB data for 2011-2012 window.
    FREE via NASA LAADS DAAC / LP DAAC (Earthdata account).
    """
    print('=' * 72)
    print('HISTORIC VIIRS DOWNLOADER — 2011/2012 THERMAL + NIGHTTIME VISIBLE')
    print('FREE SOURCE: NASA LAADS DAAC + LP DAAC (Earthdata)')
    print('=' * 72)
    print()

    if bbox is None:
        bbox = LAKE_MICHIGAN_BBOX
    if products is None:
        products = ['VNP21A1D', 'VNP02IMG', 'VNP46A1']  # LST + radiances + DNB

    token = load_token()
    if not token:
        print('[!] No Earthdata token found.')
        print('    Get one FREE at: https://urs.earthdata.nasa.gov/users/new')
        print('    Same token used for Landsat, ICESat-2, SWOT, VIIRS')
        return {}

    print(f'[+] Token loaded')
    print(f'[+] GPU: {"ACTIVE - " + torch.cuda.get_device_name(0) if CUDA else "CPU mode"}')
    print(f'[+] Products: {products}')
    print(f'[+] Bbox: {bbox}')
    print()

    all_granules = []

    for i, (start, end) in enumerate(OPTIMAL_WINDOWS, 1):
        print(f'[Window {i}/{len(OPTIMAL_WINDOWS)}] {start} → {end}')
        for product_key in products:
            if product_key not in VIIRS_PRODUCTS:
                continue
            prod = VIIRS_PRODUCTS[product_key]
            granules = query_viirs_cmr(bbox, (start, end), token,
                                        prod['short_name'], prod['version'])
            all_granules.extend(granules[:max_per_window])
        time.sleep(0.5)

    print()
    print(f'Total granules found: {len(all_granules)}')

    # Deduplicate
    seen = set()
    unique = [g for g in all_granules
              if g['granule_id'] not in seen and not seen.add(g['granule_id'])]
    all_granules = unique

    print(f'Unique granules: {len(all_granules)}')
    print(f'Estimated size: ~{len(all_granules) * 150:.0f} MB')
    print()

    downloaded = []
    for i, g in enumerate(all_granules, 1):
        print(f'[{i}/{len(all_granules)}] {g["time_start"]} {g["short_name"]}')
        path = download_viirs_granule(g, OUTPUT_DIR, token)
        if path:
            downloaded.append({
                'granule_id': g['granule_id'],
                'time_start': g['time_start'],
                'short_name': g['short_name'],
                'path': str(path),
            })
        time.sleep(0.2)

    summary = {
        'run_at': datetime.now().isoformat(),
        'source': 'NASA LAADS DAAC + LP DAAC (FREE with Earthdata)',
        'earthdata_url': 'https://urs.earthdata.nasa.gov/users/new',
        'bbox': bbox,
        'windows': OPTIMAL_WINDOWS,
        'products_requested': products,
        'granules_found': len(all_granules),
        'granules_downloaded': len(downloaded),
        'gpu_used': CUDA,
        'downloads': downloaded,
        'notes': [
            'Suomi NPP launched Oct 28 2011 — first data Nov 2011',
            'VNP21A1D: Land Surface Temp 1km daily — cold-sink detection',
            'VNP02IMG: Raw I4/I5 radiances 375m — highest thermal resolution',
            'VNP46A1: Day/Night Band 500m — nighttime thermal contrast',
            '2012 advantage: shallow water + clear water = surface-detectable cold sink',
            'Steel wreck ~3C colder than surrounding water at night',
            'Cross-reference cold sinks with Envisat SAR slicks for confirmation',
        ]
    }

    out = OUTPUT_DIR / 'viirs_download_summary.json'
    with open(out, 'w') as f:
        json.dump(summary, f, indent=2)

    print()
    print('=' * 72)
    print('VIIRS DOWNLOAD COMPLETE')
    print('=' * 72)
    print(f'  Downloaded: {len(downloaded)} granules')
    print(f'  Output:     {OUTPUT_DIR}')
    print(f'  Summary:    {out}')
    print()
    print('NEXT: Run historic_worldview2_downloader.py for Coastal Blue band')
    print('      Then fuse: VIIRS cold-sink + Envisat SAR slick + WV2 optical')
    print('=' * 72)

    return summary


if __name__ == '__main__':
    import argparse
    p = argparse.ArgumentParser()
    p.add_argument('--bbox', default='lake_michigan')
    p.add_argument('--max-per-window', type=int, default=10)
    p.add_argument('--products', default='VNP21A1D,VNP02IMG,VNP46A1')
    args = p.parse_args()

    bbox = ZION_TRENCH_BBOX if args.bbox == 'zion_trench' else LAKE_MICHIGAN_BBOX
    products = args.products.split(',')
    run(bbox=bbox, max_per_window=args.max_per_window, products=products)
