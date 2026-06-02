"""
straits_south_fox_historical_pull.py

TARGET AREA : Straits of Mackinac -> South Fox Island
BBOX        : -86.10 to -85.35 lon, 45.35 to 46.00 lat
LOW WATER   : 2007, 2012, 2013
SENSORS     : Landsat 7 ETM+, Landsat 5 TM, Suomi NPP VIIRS LST, VIIRS DNB
TILES       : 3 per sensor per month
FREE SOURCE : NASA Earthdata CMR (token already on disk)
"""

import json
import time
from datetime import datetime, timedelta
from pathlib import Path
import requests

# ── BBOX ──────────────────────────────────────────────────────────────────────
BBOX = {
    'lon_min': -86.10,
    'lat_min': 45.35,
    'lon_max': -85.35,
    'lat_max': 46.00,
    'description': 'Straits of Mackinac to South Fox Island - N Lake Michigan',
}

# ── LOW WATER WINDOWS ─────────────────────────────────────────────────────────
LOW_WATER_WINDOWS = [
    {'year': 2012, 'label': '2012_gold',   'months': [4,5,6,7,8,9,10], 'priority': 1},
    {'year': 2013, 'label': '2013_low',    'months': [4,5,6,7,8,9,10], 'priority': 2},
    {'year': 2007, 'label': '2007_low',    'months': [5,6,7,8,9],       'priority': 3},
    {'year': 2011, 'label': '2011_viirs',  'months': [11,12],            'priority': 4},
]

TILES_PER_MONTH = 3

# ── CONFIG ────────────────────────────────────────────────────────────────────
REPO       = Path(__file__).resolve().parent
OUTPUT_ROOT = REPO / 'outputs' / 'straits_south_fox_historical'
OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)

TOKEN_PATH = Path('C:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json')
CMR_BASE   = 'https://cmr.earthdata.nasa.gov/search/granules.json'
STAC_BASE  = 'https://earth-search.aws.element84.com/v1/search'

# ── SENSORS ───────────────────────────────────────────────────────────────────
# Landsat 7+5 -> Element84 STAC (landsat-c2-l2, no auth needed)
# VIIRS LST   -> NASA CMR short_name=VNP21A1D version=002  (already working)
# VIIRS DNB   -> NASA CMR short_name=VNP46A1  version=2    (confirmed working)
SENSORS = {
    # LANDSAT STATUS: STAC metadata works (Element84), scenes found with good cloud cover.
    # Download blocked: USGS C2 L2 bands require ERS username+password (separate from Earthdata JWT).
    # TO UNLOCK: Add USGS_ERS_USER and USGS_ERS_PASS to token file, or use M2M API key.
    # Token file: Bagrecovery/sentinel_hunt/earthdata_token.json
    # Add fields: {"usgs_ers_user": "YOUR_USER", "usgs_ers_pass": "YOUR_PASS"}
    # Register free at: https://ers.cr.usgs.gov/register
    # Once added, download_stac_bands() will use M2M session token automatically.
    'landsat7': {
        'source'      : 'stac',
        'collection'  : 'landsat-c2-l2',
        'platform'    : 'landsat-7',
        'label'       : 'Landsat 7 ETM+ SLC-off C2 L2',
        'years'       : [2007, 2012, 2013],
        'output_subdir': 'landsat7',
        'max_cloud'   : 35,
        'note'        : 'METADATA READY - needs USGS ERS login to download bands',
    },
    'landsat5': {
        'source'      : 'stac',
        'collection'  : 'landsat-c2-l2',
        'platform'    : 'landsat-5',
        'label'       : 'Landsat 5 TM C2 L2 (clean sensor)',
        'years'       : [2007, 2012],
        'output_subdir': 'landsat5',
        'max_cloud'   : 30,
        'note'        : 'METADATA READY - needs USGS ERS login to download bands',
    },
    'viirs_lst': {
        'source'      : 'cmr',
        'short_name'  : 'VNP21A1D',
        'version'     : '002',
        'label'       : 'Suomi NPP VIIRS LST 1km Daily',
        'years'       : [2012, 2013],
        'output_subdir': 'viirs_lst',
        'max_cloud'   : 100,
        'note'        : 'Cold-sink detection. Launched Oct 28 2011.',
        # VIIRS sinusoidal tile h09v04 covers lon -90 to -80, lat 40 to 50
        # This is the correct tile for our bbox (-86.1 to -85.35 lon)
        'tile_filter' : 'h09v04',
    },
    'viirs_dnb': {
        'source'      : 'cmr',
        'short_name'  : 'VNP46A1',
        'version'     : '2',
        'label'       : 'Suomi NPP VIIRS DNB 500m Nightly',
        'years'       : [2012, 2013],
        'output_subdir': 'viirs_dnb',
        'max_cloud'   : 100,
        'note'        : 'Nighttime visible - max thermal contrast at night.',
        'tile_filter' : 'h09v04',
    },
}

# ── AUTH ──────────────────────────────────────────────────────────────────────
def load_token():
    if TOKEN_PATH.exists():
        try:
            return json.loads(TOKEN_PATH.read_text()).get('earthdata_token', '')
        except Exception:
            pass
    return ''

# ── STAC QUERY (Element84 - no auth, Landsat C2 L2) ──────────────────────────
def query_stac(collection, platform, start, end, max_cloud=35):
    payload = {
        'collections': [collection],
        'datetime'   : f'{start}T00:00:00Z/{end}T23:59:59Z',
        'bbox'       : [BBOX['lon_min'], BBOX['lat_min'],
                        BBOX['lon_max'], BBOX['lat_max']],
        'limit'      : 50,
        'query'      : {'eo:cloud_cover': {'lte': max_cloud},
                        'platform'      : {'eq': platform}},
    }
    try:
        resp = requests.post(STAC_BASE, json=payload, timeout=30)
        resp.raise_for_status()
        features = resp.json().get('features', [])
        results = []
        for f in features:
            p    = f.get('properties', {})
            assets = f.get('assets', {})
            # Prefer SR_B4 (NIR) or ST_B6 (thermal) direct COG links
            dl_url = None
            for band in ('SR_B4', 'ST_B6', 'SR_B3', 'SR_B2', 'SR_B1'):
                if band in assets:
                    dl_url = assets[band].get('href')
                    break
            if not dl_url:
                # fallback: any asset href
                dl_url = next((a.get('href') for a in assets.values()
                               if a.get('href','').startswith('https://')), None)
            results.append({
                'granule_id' : f['id'],
                'title'      : f['id'],
                'producer_id': f['id'],
                'time_start' : p.get('datetime', '')[:10],
                'cloud_cover': p.get('eo:cloud_cover', 100),
                'platform'   : p.get('platform', ''),
                'dl_url'     : dl_url,
                'assets'     : {k: v.get('href') for k, v in assets.items()
                                if v.get('href','').startswith('https://')},
                'short_name' : collection,
            })
        return results
    except Exception as ex:
        print(f'      STAC error: {ex}')
        return []

# ── CMR QUERY (NASA Earthdata - VIIRS) ────────────────────────────────────────
def query_cmr(short_name, version, start, end, token, max_cloud=100):
    headers = {
        'Accept'       : 'application/json',
        'Authorization': f'Bearer {token}',
    }
    params = {
        'short_name'   : short_name,
        'version'      : version,
        'temporal'     : f'{start}T00:00:00Z,{end}T23:59:59Z',
        'bounding_box' : (f"{BBOX['lon_min']},{BBOX['lat_min']},"
                          f"{BBOX['lon_max']},{BBOX['lat_max']}"),
        'page_size'    : 50,
        'sort_key'     : 'start_date',
    }
    try:
        resp = requests.get(CMR_BASE, params=params, headers=headers, timeout=45)
        resp.raise_for_status()
        entries = resp.json().get('feed', {}).get('entry', [])
        results = []
        for e in entries:
            cloud = 100.0
            for attr in e.get('additional_attributes', []):
                if attr.get('name') in ('CLOUD_COVER', 'CloudCover'):
                    try:
                        cloud = float(attr.get('values', [100])[0])
                    except Exception:
                        pass
            if cloud > max_cloud:
                continue
            links  = e.get('links', [])
            dl_url = next(
                (l['href'] for l in links
                 if l.get('rel') == 'http://esipfed.org/ns/fedsearch/1.1/data#'
                 and l.get('type','') == 'application/x-hdf5'
                 and l.get('href','').startswith('https://')),
                None
            )
            if not dl_url:
                dl_url = next(
                    (l['href'] for l in links
                     if l.get('rel') == 'http://esipfed.org/ns/fedsearch/1.1/data#'
                     and l.get('href','').startswith('https://')),
                    None
                )
            results.append({
                'granule_id' : e.get('id', ''),
                'title'      : e.get('title', ''),
                'producer_id': e.get('producer_granule_id', ''),
                'time_start' : e.get('time_start', '')[:10],
                'cloud_cover': cloud,
                'dl_url'     : dl_url,
                'short_name' : short_name,
            })
        return results
    except Exception as ex:
        print(f'      CMR error: {ex}')
        return []


def pick_best(granules, n=TILES_PER_MONTH, tile_filter=None):
    # Apply tile filter if specified (e.g. 'h09v04' for correct VIIRS tile)
    if tile_filter:
        granules = [g for g in granules if tile_filter in g.get('title','')]
    sorted_g  = sorted(granules, key=lambda x: x['cloud_cover'])
    selected  = []
    seen_dates = set()
    for g in sorted_g:
        if g['time_start'] not in seen_dates:
            selected.append(g)
            seen_dates.add(g['time_start'])
        if len(selected) >= n:
            break
    for g in sorted_g:
        if len(selected) >= n:
            break
        if g not in selected:
            selected.append(g)
    return selected[:n]


def download_granule(granule, output_dir, token):
    # For STAC Landsat: download all key bands (NIR, Thermal, Red, Green, Blue)
    if granule.get('assets'):
        return download_stac_bands(granule, output_dir)

    if not granule.get('dl_url'):
        return {'status': 'no_url'}

    pid = granule.get('producer_id') or granule['title'].split('/')[-1]
    url = granule['dl_url']
    ext = '.dat'
    for e in ('.h5', '.hdf', '.HDF5', '.tar', '.nc', '.tif', '.TIF'):
        if url.lower().endswith(e.lower()):
            ext = e
            break
    if not pid.lower().endswith(ext.lower()):
        pid = pid.rstrip('.') + ext

    out_path = output_dir / pid
    if out_path.exists() and out_path.stat().st_size > 50_000:
        return {'status': 'exists', 'path': str(out_path),
                'size_mb': round(out_path.stat().st_size / 1e6, 1)}

    headers = {'Authorization': f'Bearer {token}'}
    try:
        resp = requests.get(url, headers=headers, timeout=600, stream=True)
        resp.raise_for_status()
        with open(out_path, 'wb') as f:
            for chunk in resp.iter_content(chunk_size=65536):
                if chunk:
                    f.write(chunk)
        size_mb = round(out_path.stat().st_size / 1e6, 1)
        return {'status': 'downloaded', 'path': str(out_path), 'size_mb': size_mb}
    except Exception as ex:
        if out_path.exists():
            out_path.unlink()
        return {'status': 'failed', 'error': str(ex)[:120]}


def get_usgs_m2m_token():
    """
    Get USGS M2M session token using ERS credentials stored in token file.
    Token file needs: {"usgs_ers_user": "...", "usgs_ers_pass": "..."}
    Register free at: https://ers.cr.usgs.gov/register
    Returns session token string or None if credentials not present.
    """
    try:
        creds = json.loads(TOKEN_PATH.read_text())
        user  = creds.get('usgs_ers_user', '')
        pw    = creds.get('usgs_ers_pass', '')
        if not user or not pw:
            return None
        resp = requests.post(
            'https://m2m.cr.usgs.gov/api/api/json/stable/login',
            json={'username': user, 'password': pw},
            timeout=20
        )
        resp.raise_for_status()
        token = resp.json().get('data')
        return token
    except Exception:
        return None


def download_stac_bands(granule, output_dir):
    """
    Download priority bands from a Landsat STAC item via USGS M2M.
    Bands: nir08=NIR, lwir=Thermal, red, green, blue
    S3 paths converted to USGS LandsatLook HTTPS with M2M session token.

    BLOCKED until usgs_ers_user + usgs_ers_pass added to token file.
    Register free: https://ers.cr.usgs.gov/register
    """
    m2m_token = get_usgs_m2m_token()
    if not m2m_token:
        return {
            'status': 'blocked',
            'error' : 'USGS ERS credentials needed. Add usgs_ers_user + usgs_ers_pass to earthdata_token.json. Register free at https://ers.cr.usgs.gov/register'
        }

    # Map STAC asset keys to band labels
    priority_bands = ['nir08', 'lwir', 'red', 'green', 'blue']
    assets   = granule.get('assets', {})
    scene_id = granule['granule_id']
    total_mb = 0.0
    downloaded_bands = []

    for band in priority_bands:
        s3_path = assets.get(band, '')
        if not s3_path:
            continue
        # Convert s3://usgs-landsat/... to HTTPS via LandsatLook
        https_url = s3_path.replace(
            's3://usgs-landsat',
            'https://landsatlook.usgs.gov/data'
        )
        # Append .TIF if missing
        if not https_url.lower().endswith('.tif'):
            https_url += '.TIF'

        fname    = f'{scene_id}_{band}.TIF'
        out_path = output_dir / fname
        if out_path.exists() and out_path.stat().st_size > 10_000:
            total_mb += out_path.stat().st_size / 1e6
            downloaded_bands.append(band)
            continue
        try:
            resp = requests.get(
                https_url,
                headers={'X-Auth-Token': m2m_token},
                timeout=300, stream=True
            )
            resp.raise_for_status()
            with open(out_path, 'wb') as f:
                for chunk in resp.iter_content(chunk_size=65536):
                    if chunk:
                        f.write(chunk)
            total_mb += out_path.stat().st_size / 1e6
            downloaded_bands.append(band)
        except Exception as ex:
            print(f'        band {band} failed: {str(ex)[:60]}')

    if downloaded_bands:
        return {'status': 'downloaded', 'path': str(output_dir / scene_id),
                'size_mb': round(total_mb, 1), 'bands': downloaded_bands}
    return {'status': 'failed', 'error': 'no bands downloaded'}

# ── MAIN ──────────────────────────────────────────────────────────────────────
def run():
    print('=' * 76)
    print('STRAITS OF MACKINAC -> SOUTH FOX ISLAND  HISTORICAL DATA PULL')
    print('=' * 76)
    print(f'  BBOX    : {BBOX["lon_min"]} to {BBOX["lon_max"]} lon  |  '
          f'{BBOX["lat_min"]} to {BBOX["lat_max"]} lat')
    print(f'  Sensors : {", ".join(SENSORS.keys())}')
    print(f'  Tiles   : {TILES_PER_MONTH} per sensor per month')
    print()

    token = load_token()
    if not token:
        print('[!] No Earthdata token at:')
        print(f'    {TOKEN_PATH}')
        return

    print('[+] Earthdata token loaded')
    print()

    master_log       = []
    total_downloaded = 0
    total_size_mb    = 0.0
    total_skipped    = 0
    total_failed     = 0
    total_blocked    = 0

    for sensor_key, sensor in SENSORS.items():
        sensor_dir = OUTPUT_ROOT / sensor['output_subdir']
        sensor_dir.mkdir(exist_ok=True)

        print('=' * 76)
        print(f'SENSOR: {sensor["label"]}')
        print(f'  {sensor["note"]}')
        print()

        sensor_log = []

        for window in LOW_WATER_WINDOWS:
            year = window['year']
            if year not in sensor['years']:
                continue

            print(f'  [{year}] {window["label"]}')

            for month in window['months']:
                start_dt = datetime(year, month, 1)
                end_dt   = (datetime(year, month + 1, 1) - timedelta(days=1)
                            if month < 12 else datetime(year, 12, 31))
                start_str = start_dt.strftime('%Y-%m-%d')
                end_str   = end_dt.strftime('%Y-%m-%d')

                if sensor['source'] == 'stac':
                    granules = query_stac(
                        sensor['collection'], sensor['platform'],
                        start_str, end_str,
                        max_cloud=sensor['max_cloud'],
                    )
                else:
                    granules = query_cmr(
                        sensor['short_name'], sensor['version'],
                        start_str, end_str, token,
                        max_cloud=sensor['max_cloud'],
                    )

                if not granules:
                    print(f'    {year}-{month:02d}: 0 granules')
                    continue

                selected = pick_best(granules, TILES_PER_MONTH,
                                     tile_filter=sensor.get('tile_filter'))
                clouds   = [str(round(g['cloud_cover'])) + 'pct' for g in selected]
                print(f'    {year}-{month:02d}: {len(granules)} found -> '
                      f'{len(selected)} selected  cloud={clouds}')

                for g in selected:
                    result = download_granule(g, sensor_dir, token)
                    status = result['status']

                    if status == 'downloaded':
                        total_downloaded += 1
                        total_size_mb    += result.get('size_mb', 0)
                        print(f'      + {g["time_start"]}  {result["size_mb"]} MB')
                    elif status == 'exists':
                        total_skipped += 1
                        print(f'      = {g["time_start"]}  already on disk')
                    elif status == 'blocked':
                        total_blocked += 1
                        if total_blocked == 1:  # print once per sensor
                            print(f'      ! BLOCKED: {result.get("error","")[:80]}')
                        else:
                            print(f'      ! {g["time_start"]}  blocked (credentials needed)')
                    elif status == 'no_url':
                        total_failed += 1
                        print(f'      - {g["time_start"]}  no download URL')
                    else:
                        total_failed += 1
                        print(f'      X {g["time_start"]}  FAILED: '
                              f'{result.get("error","")[:60]}')

                    entry = {
                        'sensor'     : sensor_key,
                        'year'       : year,
                        'month'      : month,
                        'granule_id' : g['granule_id'],
                        'date'       : g['time_start'],
                        'cloud_cover': g['cloud_cover'],
                    }
                    entry.update(result)
                    sensor_log.append(entry)
                    master_log.append(entry)
                    time.sleep(0.25)

            print()

        log_path = sensor_dir / f'{sensor_key}_pull_log.json'
        with open(log_path, 'w') as f:
            json.dump({'sensor': sensor_key, 'label': sensor['label'],
                       'bbox': BBOX, 'pulled_at': datetime.now().isoformat(),
                       'entries': sensor_log}, f, indent=2)
        print(f'  Log: {log_path.name}')
        print()

    # Summary
    summary = {
        'pulled_at'          : datetime.now().isoformat(),
        'bbox'               : BBOX,
        'sensors'            : list(SENSORS.keys()),
        'low_water_years'    : [w['year'] for w in LOW_WATER_WINDOWS],
        'tiles_per_month'    : TILES_PER_MONTH,
        'total_downloaded'   : total_downloaded,
        'total_skipped'      : total_skipped,
        'total_failed'       : total_failed,
        'total_size_mb'      : round(total_size_mb, 1),
        'total_size_gb'      : round(total_size_mb / 1024, 2),
        'output_root'        : str(OUTPUT_ROOT),
        'entries'            : master_log,
        'next_steps'         : [
            'Wire outputs into gpu_curvelets.py for curvelet transform',
            'Run cuda_slc_gap_fill() on Landsat 7 stacks per month',
            'Run cuda_detect_cold_sinks() on VIIRS LST tiles',
            'Cross-reference optical hits with Envisat SAR slicks',
            'Feed confirmed hits into great_lakes_scanner.py full run',
        ],
    }

    summary_path = OUTPUT_ROOT / 'straits_south_fox_pull_summary.json'
    with open(summary_path, 'w') as f:
        json.dump(summary, f, indent=2)

    print('=' * 76)
    print('PULL COMPLETE')
    print('=' * 76)
    print(f'  Downloaded : {total_downloaded} files')
    print(f'  On disk    : {total_skipped} files')
    print(f'  Blocked    : {total_blocked} files  (Landsat - needs USGS ERS login)')
    print(f'  Failed     : {total_failed} files')
    print(f'  Total size : {total_size_mb:.1f} MB  ({total_size_mb/1024:.2f} GB)')
    if total_blocked > 0:
        print()
        print('TO UNLOCK LANDSAT DOWNLOADS:')
        print('  1. Register free at https://ers.cr.usgs.gov/register')
        print('  2. Add to earthdata_token.json:')
        print('     "usgs_ers_user": "your_username"')
        print('     "usgs_ers_pass": "your_password"')
        print('  3. Re-run this script - Landsat will download automatically')
    print(f'  Output     : {OUTPUT_ROOT}')
    print(f'  Summary    : {summary_path}')
    print()
    print('NEXT STEPS:')
    for step in summary['next_steps']:
        print(f'  -> {step}')
    print('=' * 76)


if __name__ == '__main__':
    run()
