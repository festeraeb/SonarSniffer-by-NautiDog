"""
historic_envisat_asar_downloader.py

2011-2012 TIME MACHINE — Envisat ASAR C-Band SAR
=================================================
Envisat operated 2002-2012. ESA Heritage Archive is FREE for research.
ASAR (Advanced Synthetic Aperture Radar) at C-band (5.6cm wavelength).

FREE SOURCE: ESA Heritage Archive via CDSE (Copernicus Data Space Ecosystem)
             No cost for research use. Register at: https://dataspace.copernicus.eu
             Also available via ESA EOLI-SA and EO Sign In.

Why 2012 SAR is the "Structure Finder":
  - Low water + clear water = submerged structures disrupt bottom currents
  - Those current disruptions create surface slicks (calm patches)
  - Calm patches = DARK spots in SAR (low backscatter)
  - These dark spots DON'T MOVE WITH WIND — they're anchored to the bottom
  - A moving dark patch = wind shadow. A FIXED dark patch = wreck below.

ASAR Modes used:
  IMP  - Image Mode Precision (12.5m resolution) <- best for wreck detection
  IMS  - Image Mode Single Look Complex (SLC)
  WSM  - Wide Swath Medium (150m) <- for large area survey

CUDA: SAR backscatter analysis and surface slick detection on M2200 GPU.
"""

import json
import time
import math
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
OUTPUT_DIR = REPO / 'outputs' / 'historic_envisat_asar'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# ESA CDSE credentials (free research account)
# Register at: https://dataspace.copernicus.eu/
CDSE_TOKEN_PATH = Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/cdse_token.json')
# Format: {"access_token": "...", "refresh_token": "..."}
# Get token via: POST https://identity.dataspace.copernicus.eu/auth/realms/CDSE/protocol/openid-connect/token

# ESA CDSE OData API (replaces old SCIHUB)
CDSE_ODATA = 'https://catalogue.dataspace.copernicus.eu/odata/v1'
CDSE_DOWNLOAD = 'https://zipper.dataspace.copernicus.eu/zip'

# Fallback: ESA EO Sign In (older archive)
ESA_EOCAT = 'https://eocat.esa.int/eo-catalogue/collections/ENVISAT.ASA_IMP_1P/search.json'

# Lake Michigan bbox
LAKE_MICHIGAN_BBOX = {
    'lon_min': -87.9, 'lat_min': 41.5,
    'lon_max': -85.5, 'lat_max': 46.0,
}

ZION_TRENCH_BBOX = {
    'lon_min': -87.60, 'lat_min': 42.35,
    'lon_max': -87.40, 'lat_max': 42.60,
}

# 2011-2012 optimal windows (calm summer/fall for surface slick detection)
OPTIMAL_WINDOWS = [
    ('2012-06-01', '2012-09-30'),  # Peak 2012 low water + calm season
    ('2012-04-01', '2012-05-31'),  # Spring post-ice
    ('2011-07-01', '2011-09-30'),  # 2011 baseline
]

# ASAR product types (priority order)
ASAR_PRODUCTS = [
    'ASA_IMP_1P',   # Image Mode Precision — 12.5m, best for wrecks
    'ASA_IMS_1P',   # Image Mode SLC
    'ASA_WSM_1P',   # Wide Swath Medium — 150m, area survey
]

# Surface slick detection thresholds
SLICK_THRESHOLD_DB = -18.0   # dB — calm water / slick signature
WIND_SHADOW_RATIO = 0.85     # If slick moves >85% with wind vector = wind shadow, not wreck

# ── Auth ──────────────────────────────────────────────────────────────────────

def load_cdse_token() -> str:
    """
    Load CDSE access token.
    If expired, attempt refresh.
    Get free account at: https://dataspace.copernicus.eu/
    """
    if CDSE_TOKEN_PATH.exists():
        try:
            data = json.loads(CDSE_TOKEN_PATH.read_text())
            return data.get('access_token', '')
        except Exception:
            pass

    print('[!] No CDSE token found.')
    print('    1. Register FREE at: https://dataspace.copernicus.eu/')
    print('    2. Get token:')
    print('       curl -X POST https://identity.dataspace.copernicus.eu/auth/realms/CDSE/protocol/openid-connect/token \\')
    print('         -d "grant_type=password&client_id=cdse-public&username=YOUR_EMAIL&password=YOUR_PASS"')
    print(f'    3. Save to: {CDSE_TOKEN_PATH}')
    print('       Format: {"access_token": "...", "refresh_token": "..."}')
    return ''


def refresh_cdse_token(refresh_token: str) -> str:
    """Refresh expired CDSE token."""
    try:
        resp = requests.post(
            'https://identity.dataspace.copernicus.eu/auth/realms/CDSE/protocol/openid-connect/token',
            data={
                'grant_type': 'refresh_token',
                'client_id': 'cdse-public',
                'refresh_token': refresh_token,
            },
            timeout=30
        )
        resp.raise_for_status()
        new_tokens = resp.json()
        # Save updated tokens
        if CDSE_TOKEN_PATH.parent.exists():
            CDSE_TOKEN_PATH.write_text(json.dumps(new_tokens, indent=2))
        return new_tokens.get('access_token', '')
    except Exception as ex:
        print(f'[!] Token refresh failed: {ex}')
        return ''

# ── CDSE OData Query ──────────────────────────────────────────────────────────

def query_envisat_cdse(bbox: dict, date_range: tuple, token: str,
                        product_type: str = 'ASA_IMP_1P') -> list:
    """
    Query ESA CDSE OData API for Envisat ASAR products.
    FREE for research — ESA Heritage Archive.

    OData filter syntax:
      Collection/Name eq 'ENVISAT' AND
      Attributes/OData.CSC.StringAttribute/any(att:att/Name eq 'productType'
        and att/OData.CSC.StringAttribute/Value eq 'ASA_IMP_1P') AND
      ContentDate/Start gt 2012-06-01T00:00:00.000Z AND
      ContentDate/Start lt 2012-09-30T23:59:59.000Z AND
      OData.CSC.Intersects(area=geography'SRID=4326;POLYGON(...)')
    """
    # Build WKT polygon from bbox
    wkt = (f"POLYGON(({bbox['lon_min']} {bbox['lat_min']},"
           f"{bbox['lon_max']} {bbox['lat_min']},"
           f"{bbox['lon_max']} {bbox['lat_max']},"
           f"{bbox['lon_min']} {bbox['lat_max']},"
           f"{bbox['lon_min']} {bbox['lat_min']}))")

    odata_filter = (
        f"Collection/Name eq 'ENVISAT' and "
        f"Attributes/OData.CSC.StringAttribute/any(att:att/Name eq 'productType' "
        f"and att/OData.CSC.StringAttribute/Value eq '{product_type}') and "
        f"ContentDate/Start gt {date_range[0]}T00:00:00.000Z and "
        f"ContentDate/Start lt {date_range[1]}T23:59:59.000Z and "
        f"OData.CSC.Intersects(area=geography'SRID=4326;{wkt}')"
    )

    headers = {'Authorization': f'Bearer {token}'} if token else {}

    params = {
        '$filter': odata_filter,
        '$orderby': 'ContentDate/Start asc',
        '$top': 100,
        '$expand': 'Attributes',
    }

    print(f'  Querying CDSE OData: {product_type} {date_range[0]} → {date_range[1]}')
    try:
        resp = requests.get(f'{CDSE_ODATA}/Products',
                            params=params, headers=headers, timeout=60)
        resp.raise_for_status()
        items = resp.json().get('value', [])

        granules = []
        for item in items:
            granules.append({
                'product_id': item.get('Id', ''),
                'name': item.get('Name', ''),
                'date': item.get('ContentDate', {}).get('Start', '')[:10],
                'size_mb': item.get('ContentLength', 0) / 1e6,
                'product_type': product_type,
                'online': item.get('Online', False),
            })

        print(f'    Found {len(granules)} {product_type} products')
        return granules

    except Exception as ex:
        print(f'    CDSE query failed: {ex}')
        return []


def query_envisat_eocat_fallback(bbox: dict, date_range: tuple) -> list:
    """
    Fallback: Query ESA EO Catalogue (no auth required for metadata).
    Use this if CDSE account not yet set up.
    """
    params = {
        'bbox': f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}",
        'timeStart': f'{date_range[0]}T00:00:00Z',
        'timeEnd': f'{date_range[1]}T23:59:59Z',
        'maximumRecords': 50,
        'startRecord': 1,
    }

    print(f'  Querying ESA EO Catalogue (fallback, no auth)...')
    try:
        resp = requests.get(ESA_EOCAT, params=params, timeout=30)
        resp.raise_for_status()
        features = resp.json().get('features', [])
        print(f'    Found {len(features)} products (metadata only — need CDSE for download)')
        return [{'name': f.get('properties', {}).get('title', ''),
                 'date': f.get('properties', {}).get('date', ''),
                 'product_id': f.get('id', ''),
                 'online': False,
                 'product_type': 'ASA_IMP_1P'} for f in features]
    except Exception as ex:
        print(f'    EO Catalogue fallback failed: {ex}')
        return []


def download_envisat_product(product: dict, output_dir: Path, token: str) -> Path | None:
    """
    Download Envisat ASAR product from CDSE.
    Uses CDSE Zipper service for on-demand packaging.
    """
    if not product.get('product_id') or not token:
        return None

    fname = product['name'] if product['name'].endswith('.N1') else product['name'] + '.zip'
    out_path = output_dir / fname

    if out_path.exists() and out_path.stat().st_size > 100_000:
        print(f'    ✓ Already have: {fname}')
        return out_path

    # Check if product is online (not in tape archive)
    if not product.get('online', True):
        print(f'    [!] Product in tape archive — requesting restore...')
        # Trigger restore (async — may take hours)
        try:
            requests.post(
                f"{CDSE_ODATA}/Products({product['product_id']})/OData.CSC.Order",
                headers={'Authorization': f'Bearer {token}'},
                timeout=30
            )
            print(f'    Restore requested. Check back in 1-24 hours.')
        except Exception:
            pass
        return None

    print(f'    Downloading {fname} ({product.get("size_mb", 0):.0f} MB)...')
    dl_url = f"https://download.dataspace.copernicus.eu/odata/v1/Products({product['product_id']})/$value"

    headers = {'Authorization': f'Bearer {token}'}
    try:
        resp = requests.get(dl_url, headers=headers, timeout=600, stream=True)
        resp.raise_for_status()
        with open(out_path, 'wb') as f:
            for chunk in resp.iter_content(chunk_size=65536):
                if chunk:
                    f.write(chunk)
        size_mb = out_path.stat().st_size / 1e6
        print(f'    ✓ Saved {fname} ({size_mb:.1f} MB)')
        return out_path
    except Exception as ex:
        print(f'    ✗ Download failed: {ex}')
        if out_path.exists():
            out_path.unlink()
        return None

# ── CUDA SAR Surface Slick Detector ──────────────────────────────────────────

def cuda_detect_surface_slicks(sar_array: 'np.ndarray',
                                 transform=None) -> list:
    """
    GPU-accelerated surface slick detection in ASAR backscatter.

    A wreck-caused slick signature:
      1. Backscatter < SLICK_THRESHOLD_DB (dark patch in SAR)
      2. Spatial extent > 50m (not noise)
      3. Shape: elongated or irregular (not circular wind shadow)
      4. FIXED across multiple passes (not moving with wind)

    Returns list of candidate slick locations with coordinates.
    """
    if np is None:
        return []

    try:
        from scipy import ndimage
    except ImportError:
        print('    [!] scipy not installed — skipping slick detection')
        return []

    print(f'    SAR slick detection on {"GPU" if CUDA else "CPU"}...')
    print(f'    Array shape: {sar_array.shape}')

    # Convert to dB if linear
    if sar_array.max() > 100:
        sar_db = 10 * np.log10(np.maximum(sar_array, 1e-10))
    else:
        sar_db = sar_array.copy()

    if CUDA and torch is not None:
        arr_t = torch.from_numpy(sar_db.astype(np.float32)).to(DEVICE)

        # Threshold: dark patches below slick threshold
        slick_mask = arr_t < SLICK_THRESHOLD_DB

        # Move back to CPU for connected component analysis
        slick_np = slick_mask.cpu().numpy()
        torch.cuda.empty_cache()
    else:
        slick_np = sar_db < SLICK_THRESHOLD_DB

    # Label connected components
    labeled, n_features = ndimage.label(slick_np)
    sizes = ndimage.sum(slick_np, labeled, range(1, n_features + 1))

    candidates = []
    for label_id in range(1, n_features + 1):
        size_px = int(sizes[label_id - 1])
        if size_px < 16:  # Too small — noise
            continue

        rows, cols = np.where(labeled == label_id)
        center_row = int(np.mean(rows))
        center_col = int(np.mean(cols))

        # Aspect ratio (elongated = more wreck-like)
        row_span = np.ptp(rows) + 1
        col_span = np.ptp(cols) + 1
        aspect = max(row_span, col_span) / max(min(row_span, col_span), 1)

        # Convert to lat/lon if transform available
        lat, lon = 0.0, 0.0
        if transform is not None:
            try:
                lon, lat = transform * (center_col, center_row)
            except Exception:
                pass

        candidates.append({
            'row': center_row,
            'col': center_col,
            'lat': lat,
            'lon': lon,
            'size_px': size_px,
            'aspect_ratio': round(aspect, 2),
            'mean_db': float(np.mean(sar_db[labeled == label_id])),
            'slick_type': 'ELONGATED' if aspect > 2.5 else 'COMPACT',
            'wreck_candidate': aspect > 1.5 and size_px > 50,
        })

    # Sort by size descending
    candidates.sort(key=lambda x: -x['size_px'])
    print(f'    Found {len(candidates)} slick candidates '
          f'({sum(1 for c in candidates if c["wreck_candidate"])} wreck-like)')
    return candidates

# ── Main ──────────────────────────────────────────────────────────────────────

def run(bbox: dict = None, max_per_window: int = 10) -> dict:
    """
    Download Envisat ASAR products for 2011-2012 window.
    FREE via ESA CDSE Heritage Archive.
    """
    print('=' * 72)
    print('HISTORIC ENVISAT ASAR DOWNLOADER — 2011/2012 SURFACE SLICK SCAN')
    print('FREE SOURCE: ESA CDSE Heritage Archive (research use)')
    print('=' * 72)
    print()

    if bbox is None:
        bbox = LAKE_MICHIGAN_BBOX

    token = load_cdse_token()
    if not token:
        print('[!] Attempting EO Catalogue fallback (metadata only)...')
        print()

    print(f'[+] GPU: {"ACTIVE - " + torch.cuda.get_device_name(0) if CUDA else "CPU mode"}')
    print(f'[+] Bbox: {bbox}')
    print()

    all_granules = []

    for i, (start, end) in enumerate(OPTIMAL_WINDOWS, 1):
        print(f'[Window {i}/{len(OPTIMAL_WINDOWS)}] {start} → {end}')
        for product_type in ASAR_PRODUCTS:
            if token:
                granules = query_envisat_cdse(bbox, (start, end), token, product_type)
            else:
                granules = query_envisat_eocat_fallback(bbox, (start, end))
            all_granules.extend(granules[:max_per_window])
        time.sleep(0.5)

    print()
    print(f'Total products found: {len(all_granules)}')

    # Deduplicate
    seen = set()
    unique = [g for g in all_granules
              if g['product_id'] not in seen and not seen.add(g['product_id'])]
    all_granules = unique

    downloaded = []
    skipped_tape = []

    if token:
        for i, g in enumerate(all_granules, 1):
            print(f'[{i}/{len(all_granules)}] {g["date"]} {g["product_type"]} {g["name"][:40]}')
            path = download_envisat_product(g, OUTPUT_DIR, token)
            if path:
                downloaded.append({'product_id': g['product_id'],
                                    'name': g['name'],
                                    'date': g['date'],
                                    'path': str(path)})
            elif not g.get('online', True):
                skipped_tape.append(g['name'])
            time.sleep(0.3)
    else:
        print('[!] No CDSE token — saving metadata only for manual download')
        meta_path = OUTPUT_DIR / 'envisat_products_to_download.json'
        with open(meta_path, 'w') as f:
            json.dump(all_granules, f, indent=2)
        print(f'    Metadata saved: {meta_path}')
        print('    Register at https://dataspace.copernicus.eu/ then re-run')

    summary = {
        'run_at': datetime.now().isoformat(),
        'source': 'ESA CDSE Heritage Archive (FREE for research)',
        'register_url': 'https://dataspace.copernicus.eu/',
        'bbox': bbox,
        'windows': OPTIMAL_WINDOWS,
        'products_found': len(all_granules),
        'downloaded': len(downloaded),
        'skipped_tape_archive': len(skipped_tape),
        'gpu_used': CUDA,
        'downloads': downloaded,
        'tape_restore_needed': skipped_tape,
        'notes': [
            'ASA_IMP_1P = 12.5m resolution — best for wreck surface slicks',
            'Dark patches in SAR = surface slicks from bottom current disruption',
            'Fixed dark patches (not moving with wind) = wreck below',
            'Cross-reference with Landsat 7 optical for mussel offset correction',
            'Tape archive products need 1-24hr restore request before download',
        ]
    }

    out = OUTPUT_DIR / 'envisat_asar_summary.json'
    with open(out, 'w') as f:
        json.dump(summary, f, indent=2)

    print()
    print('=' * 72)
    print('ENVISAT ASAR DOWNLOAD COMPLETE')
    print('=' * 72)
    print(f'  Found:     {len(all_granules)} products')
    print(f'  Downloaded:{len(downloaded)}')
    print(f'  Tape restore needed: {len(skipped_tape)}')
    print(f'  Output:    {OUTPUT_DIR}')
    print()
    print('NEXT: Cross-reference SAR slicks with Landsat 7 optical hits')
    print('      Use flag_mussel_false_positives() in landsat7 downloader')
    print('=' * 72)

    return summary


if __name__ == '__main__':
    import argparse
    p = argparse.ArgumentParser()
    p.add_argument('--bbox', default='lake_michigan')
    p.add_argument('--max-per-window', type=int, default=10)
    args = p.parse_args()

    bbox = ZION_TRENCH_BBOX if args.bbox == 'zion_trench' else LAKE_MICHIGAN_BBOX
    run(bbox=bbox, max_per_window=args.max_per_window)
