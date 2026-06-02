"""
data_fetcher_scavenger.py

Fetches Sentinel-2 L2A COG tiles from the Element84 earth-search STAC API
(https://earth-search.aws.element84.com/v1) backed by the public AWS S3 bucket
s3://sentinel-cogs.  No NASA Earthdata token required — these are open COGs.

Yesterday's successful run used scene S2C_16TDN_20250916_0_L2A from this
source.  This fetcher pins to that exact scene and date so we get every band
for the same surface-target pass.

Band routing:
  B01, B03, B04  — 10/20 m Sentinel-2 optical (also available on L30, but
                   we use the same scene here for spatial consistency)
  B08            — 10 m NIR, Sentinel-2 only

Tiles covered by scene 16TDN (Lake Michigan / southern basin):
  The scene covers a single MGRS tile; all bands come from the same granule.
"""

import json
import os
import shutil
from pathlib import Path

import requests

# ── Auth (NASA token kept for HLS fallback path, not used for Element84) ──────

EARTHDATA_TOKEN = os.environ.get('NASA_EARTHDATA_TOKEN', '').strip()

if not EARTHDATA_TOKEN:
    _token_paths = [
        Path('c:/Users/thomf/programming/Bagrecovery/erie_remote/erie_remote_data/.earthdata_token'),
        Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
        Path('c:/Users/thomf/programming/Bagrecovery/.earthdata_token'),
        Path.home() / '.netrc',
    ]
    for _tp in _token_paths:
        if _tp.exists():
            try:
                if _tp.suffix == '.json':
                    _j = json.loads(_tp.read_text(encoding='utf-8'))
                    EARTHDATA_TOKEN = _j.get('earthdata_token', '').strip()
                else:
                    EARTHDATA_TOKEN = _tp.read_text(encoding='utf-8').strip()
                if EARTHDATA_TOKEN:
                    break
            except Exception:
                continue

# ── Constants ─────────────────────────────────────────────────────────────────

CACHE_DIR = Path('..') / 'Bagrecovery' / 'outputs' / 'rossa_forensic_cache'
CACHE_DIR.mkdir(parents=True, exist_ok=True)

# Additional places to discover already-downloaded tiles
EXTRA_CACHE_DIRS = [
    Path(r'C:/Users/thomf/programming/wreckhunter2000/data/cache/census_raw/2025_rossa'),
    Path(r'C:/Users/thomf/programming/wreckhunter2000/data/cache/census_raw/2021_low_water'),
    Path(r'C:/Users/thomf/programming/wreckhunter2000/Bagrecovery/outputs/rossa_forensic_cache'),
]

# Sentinel scene config for surface-target scan
# The primary scene is on Lake Michigan southern basin, and we also support additional tiles for northern coverage.
TARGET_SCENES = [
    {
        'scene_id': 'S2C_16TDN_20250916_0_L2A',
        'scene_date': '2025-09-16',
        'mgrs_tile': '16TDN',
        'lake': 'Michigan_16TDN',
    },
    {
        'scene_id': 'S2C_16TET_20250916_0_L2A',
        'scene_date': '2025-09-16',
        'mgrs_tile': '16TET',
        'lake': 'Michigan_16TET',
    },
    # Add more tiles here as needed for northern coverage.
]

# Default fallback scene
DEFAULT_SCENE = TARGET_SCENES[0]

# All Sentinel-2 bands we want for the surface-target scan
BANDS = ['B01', 'B02', 'B03', 'B04', 'B05', 'B06', 'B07', 'B08', 'B8A', 'B09', 'B11', 'B12']

# Element84 earth-search STAC — public, no auth
STAC_SEARCH = 'https://earth-search.aws.element84.com/v1/search'
STAC_COLLECTION = 'sentinel-2-l2a'

# Tile → lake name mapping (expand as needed)
TILE_INFO = {
    'T16TET': 'Michigan',
    'T17TLC': 'Huron',
    'T17TNE': 'Erie',
    '16TDN':  'Michigan_16TDN',  # yesterday's scene tile
}

# ── Internal helpers ──────────────────────────────────────────────────────────

def _s2_filename(scene_id: str, band: str) -> str:
    return f'{scene_id}.{band}.tif'


def _cached(scene_id: str, band: str) -> Path | None:
    filename = _s2_filename(scene_id, band)
    cache_path = CACHE_DIR / filename

    if cache_path.exists() and cache_path.stat().st_size > 0:
        return cache_path

    # Look for pre-downloaded tiles in alternate directories
    for extra_dir in EXTRA_CACHE_DIRS:
        candidate = extra_dir / filename
        if candidate.exists() and candidate.stat().st_size > 0:
            print(f'[+] Using existing tile from {candidate}')
            # Copy into CACHE_DIR for consistent usage by other pipelines
            cache_path.parent.mkdir(parents=True, exist_ok=True)
            try:
                shutil.copy2(candidate, cache_path)
                print(f'[+] Copied existing tile to cache: {cache_path}')
                return cache_path
            except Exception as e:
                print(f'[!] Failed to copy {candidate} to {cache_path}: {e}')
                return candidate

    return None


def _stac_search_scene(scene_id: str) -> dict | None:
    """Fetch the exact STAC item by scene ID from Element84."""
    # Try direct item lookup first
    item_url = f'https://earth-search.aws.element84.com/v1/collections/{STAC_COLLECTION}/items/{scene_id}'
    try:
        resp = requests.get(item_url, timeout=30)
        if resp.status_code == 200:
            return resp.json()
    except Exception as e:
        print(f'[!] Direct item lookup failed: {e}')

    # Fall back to search by scene ID
    payload = {
        'collections': [STAC_COLLECTION],
        'ids': [scene_id],
        'limit': 1,
    }
    try:
        resp = requests.post(STAC_SEARCH, json=payload, timeout=30)
        resp.raise_for_status()
        features = resp.json().get('features', [])
        return features[0] if features else None
    except Exception as e:
        print(f'[!] STAC search failed: {e}')
        return None


def _stac_search_by_date(mgrs_tile: str, date: str) -> dict | None:
    """Search for the best scene on a given date for an MGRS tile."""
    payload = {
        'collections': [STAC_COLLECTION],
        'datetime': f'{date}T00:00:00Z/{date}T23:59:59Z',
        'query': {
            'mgrs:utm_zone': {'eq': int(mgrs_tile[:2])},
            'eo:cloud_cover': {'lte': 5},
        },
        'limit': 10,
    }
    try:
        resp = requests.post(STAC_SEARCH, json=payload, timeout=30)
        resp.raise_for_status()
        features = resp.json().get('features', [])
        # prefer exact tile match
        for f in features:
            if mgrs_tile.upper() in f.get('id', '').upper():
                return f
        return features[0] if features else None
    except Exception as e:
        print(f'[!] STAC date search failed: {e}')
        return None


def _band_url_from_item(item: dict, band: str) -> str | None:
    """Extract the COG download URL for a band from a STAC item."""
    assets = item.get('assets', {})

    # Sentinel-2 STAC asset keys vary: 'B01', 'blue', 'nir', etc.
    # Try direct key match first, then common aliases
    band_aliases = {
        'B01': ['B01', 'coastal', 'B1'],
        'B02': ['B02', 'blue', 'B2'],
        'B03': ['B03', 'green', 'B3'],
        'B04': ['B04', 'red', 'B4'],
        'B05': ['B05', 'rededge', 'rededge1', 'B5'],
        'B06': ['B06', 'rededge2', 'B6'],
        'B07': ['B07', 'rededge3', 'B7'],
        'B08': ['B08', 'nir', 'B8'],
        'B8A': ['B8A', 'nir08', 'nir2'],
        'B09': ['B09', 'nir09', 'B9'],
        'B11': ['B11', 'swir16', 'swir1'],
        'B12': ['B12', 'swir22', 'swir2'],
    }
    for key in band_aliases.get(band, [band]):
        if key in assets:
            href = assets[key].get('href', '')
            if href:
                return href

    # Last resort: scan all assets for band string in href
    for asset in assets.values():
        href = asset.get('href', '')
        if f'/{band}.' in href or f'_{band}.' in href or f'.{band}.' in href:
            return href

    return None


def _download(url: str, dest: Path):
    """Download a COG from public S3 or HTTPS — no auth needed for sentinel-cogs."""
    # Convert s3:// to HTTPS if needed
    if url.startswith('s3://sentinel-cogs/'):
        url = url.replace('s3://sentinel-cogs/', 'https://sentinel-cogs.s3.us-west-2.amazonaws.com/')

    print(f'[+] Downloading {dest.name}')
    print(f'    from {url}')
    with requests.get(url, stream=True, timeout=300, allow_redirects=True) as r:
        r.raise_for_status()
        with open(dest, 'wb') as f:
            for chunk in r.iter_content(chunk_size=2 * 1024 * 1024):
                if chunk:
                    f.write(chunk)
    if dest.stat().st_size == 0:
        dest.unlink(missing_ok=True)
        raise RuntimeError(f'Zero-byte download: {dest}')
    print(f'[+] Saved {dest.name} ({dest.stat().st_size:,} bytes)')

# ── Core download ─────────────────────────────────────────────────────────────

def _download_via_nasa(scene_id: str, mgrs_tile: str, band: str, year_window: str, dest: Path) -> Path:
    """
    NASA LP DAAC / ASF fallback using HLS S30 (Sentinel-2) via CMR + earthaccess.
    Only called when Element84 STAC fails.
    B01/B03/B04 try HLS.L30 first then HLS.S30; B08 is S30-only.
    """
    if not EARTHDATA_TOKEN:
        raise RuntimeError('NASA fallback requires NASA_EARTHDATA_TOKEN')

    from datetime import datetime, timedelta

    try:
        import earthaccess
        HAS_EA = True
    except ImportError:
        HAS_EA = False

    S30_ONLY = {'B08', 'B8A', 'B05', 'B06', 'B07', 'B09'}
    windows = {
        'rossa':    ('2025-08-22T00:00:00Z', '2025-08-26T23:59:59Z'),
        'baseline': ('2024-08-22T00:00:00Z', '2024-08-26T23:59:59Z'),
    }
    date_from, date_to = windows.get(year_window, windows['rossa'])
    product_order = ['HLSS30'] if band in S30_ONLY else ['HLSL30', 'HLSS30']
    prefix_map = {'HLSL30': 'HLS.L30', 'HLSS30': 'HLS.S30'}
    auth_headers = {'Authorization': f'Bearer {EARTHDATA_TOKEN}', 'Accept': 'application/octet-stream'}

    # 1. CMR search — ASF then LP DAAC
    for short_name in product_order:
        for provider in ('ASF', 'LPCLOUD'):
            params = {
                'short_name': short_name, 'provider': provider,
                'page_size': 50, 'temporal': f'{date_from},{date_to}',
            }
            try:
                resp = requests.get('https://cmr.earthdata.nasa.gov/search/granules.json',
                                    params=params, headers={'Accept': 'application/json'}, timeout=30)
                resp.raise_for_status()
                for item in resp.json().get('feed', {}).get('entry', []):
                    if mgrs_tile.upper() in item.get('title', '').upper():
                        for link in item.get('links', []):
                            href = link.get('href', '')
                            if f'.{band}.tif' in href:
                                with requests.get(href, stream=True, headers=auth_headers,
                                                  timeout=300, allow_redirects=True) as r:
                                    r.raise_for_status()
                                    with open(dest, 'wb') as f:
                                        for chunk in r.iter_content(chunk_size=2 * 1024 * 1024):
                                            if chunk:
                                                f.write(chunk)
                                if dest.stat().st_size > 0:
                                    print(f'[+] NASA CMR ({short_name}/{provider}) saved {dest.name}')
                                    return dest
            except Exception as e:
                print(f'[!] NASA CMR {short_name}/{provider} failed: {e}')

    # 2. earthaccess
    if HAS_EA:
        fmt = '%Y-%m-%dT%H:%M:%SZ'
        try:
            try:
                earthaccess.login(strategy='environment')
            except Exception:
                earthaccess.login(strategy='netrc')
            for short_name in product_order:
                for delta in (0, 5):
                    d1 = datetime.strptime(date_from, fmt) - timedelta(days=delta)
                    d2 = datetime.strptime(date_to, fmt) + timedelta(days=delta)
                    results = earthaccess.search_data(
                        short_name=short_name,
                        temporal=(d1.strftime(fmt), d2.strftime(fmt)),
                        count=20,
                    )
                    for entry in results:
                        if mgrs_tile.upper() not in str(entry.get('title', '')).upper():
                            continue
                        files = earthaccess.download([entry], local_path=str(CACHE_DIR))
                        for f in files:
                            fp = Path(f)
                            if f'.{band}.' in fp.name and fp.stat().st_size > 0:
                                fp.rename(dest)
                                print(f'[+] NASA earthaccess saved {dest.name}')
                                return dest
                    if results:
                        break
        except Exception as e:
            print(f'[!] NASA earthaccess failed: {e}')

    raise RuntimeError(f'NASA fallback exhausted for scene={scene_id} band={band} window={year_window}')


def download_band(scene_id: str, mgrs_tile: str, band: str, year_window: str = 'rossa') -> Path:
    """
    Download one Sentinel-2 band.
    Primary:  Element84 earth-search STAC (public AWS S3 COGs, no auth)
    Fallback: NASA LP DAAC / ASF via CMR + earthaccess (requires token)
    """
    dest = CACHE_DIR / _s2_filename(scene_id, band)

    existing = _cached(scene_id, band)
    if existing:
        return existing

    # ── Primary: Element84 STAC ───────────────────────────────────────────────
    try:
        print(f'[+] Fetching STAC item for {scene_id}...')
        item = _stac_search_scene(scene_id)
        if not item:
            print(f'[!] Scene not found by ID, searching by scene date...')
            scene_date = ''
            if '_' in scene_id:
                scene_date = scene_id.split('_')[2]
            item = _stac_search_by_date(mgrs_tile, scene_date)
        if item:
            url = _band_url_from_item(item, band)
            if url:
                _download(url, dest)
                return dest
            print(f'[!] Band {band} not in STAC assets, trying NASA fallback...')
        else:
            print(f'[!] Element84 STAC returned no scene, trying NASA fallback...')
    except Exception as e:
        print(f'[!] Element84 STAC failed ({e}), trying NASA fallback...')
        if dest.exists():
            dest.unlink(missing_ok=True)

    # ── Fallback: NASA LP DAAC / ASF ──────────────────────────────────────────
    return _download_via_nasa(scene_id, mgrs_tile, band, year_window, dest)

# ── Public API ────────────────────────────────────────────────────────────────

def ensure_tiles(year_window: str = 'rossa', scene_list=None) -> dict:
    """
    Download all bands for target scenes and return {lake: {band: Path}}.
    Supports multiple tiles for full Lake Michigan coverage.
    """
    if scene_list is None:
        scene_list = TARGET_SCENES

    output_paths = {}
    for scene in scene_list:
        scene_id = scene['scene_id']
        mgrs_tile = scene['mgrs_tile']
        lake = scene['lake']

        print(f'[+] Ensuring bands for {lake} ({scene_id})')
        output_paths[lake] = {}

        for band in BANDS:
            if _cached(scene_id, band) is None:
                download_band(scene_id, mgrs_tile, band, year_window)

        for band in BANDS:
            p = _cached(scene_id, band)
            if p is None:
                raise FileNotFoundError(f'Post-download missing: {scene_id} {band}')
            output_paths[lake][band] = p

    return output_paths


if __name__ == '__main__':
    import argparse

    parser = argparse.ArgumentParser(description='Download Sentinel-2 tiles for Lake Michigan (census_raw)')
    parser.add_argument('--year-window', default='rossa', choices=['rossa', 'baseline'], help='Year window for NASA fallback')
    parser.add_argument('--tiles', default=None, help='Comma-separated MGRS tiles to download (e.g. 16TDN,16TET). If omitted, uses TARGET_SCENES list.')
    args = parser.parse_args()

    if args.tiles:
        tile_names = [t.strip().upper() for t in args.tiles.split(',') if t.strip()]
        scene_list = [s for s in TARGET_SCENES if s['mgrs_tile'].upper() in tile_names]
        if not scene_list:
            raise ValueError(f'No configured scenes match tiles: {tile_names}')
    else:
        scene_list = TARGET_SCENES

    files = ensure_tiles(year_window=args.year_window, scene_list=scene_list)
    print(json.dumps(
        {k: {b: str(p) for b, p in v.items()} for k, v in files.items()},
        indent=2,
    ))
