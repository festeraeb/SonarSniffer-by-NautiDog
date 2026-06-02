#!/usr/bin/env python3
"""
CESAROPS Aeromagnetic Data Downloader
Downloads highest-resolution publicly available aeromagnetic anomaly grids
covering all five Great Lakes (USA + Canada).

Sources:
  1. USGS mrdata.usgs.gov — North America merged grid (NAmag_origmrg, NAmag_hp500)
     - NAmag_origmrg: original merged field from all surveys, ~1 km resolution
     - NAmag_hp500: high-pass 500km filter — best for local anomaly detection (wrecks)
     - NAmag_CM: comprehensive-model reference (skip — less useful for near-surface)
     - USmag_origmrg, USmag_hp500: US-only subset
     - magnetic.xyz.gz / magnetic.gxf.gz: US XYZ/GXF formats
  2. NRCan Open Data — individual Ontario aeromag survey tiles (Great Lakes border)
     Tiles cover: Kitchener, Tobermory, Bruce, Lake Simcoe, Sault Ste. Marie, etc.

Inverse-cube law note: Satellite (MF-7, SWARM, CHAMP) data has ~100 km spatial
resolution — useless for wreck-scale (~10m) anomalies. Airborne surveys flown at
100–500m AGL give 50–400m spatial resolution. These grids are the best available.

Output: /mnt/data-external/cesarops/downloads/aeromag/
"""

import os, sys, time, json, re, hashlib
from pathlib import Path
import requests

OUT = Path('/mnt/data-external/cesarops/downloads/aeromag')
OUT.mkdir(parents=True, exist_ok=True)

UA = {'User-Agent': 'CESAROPS-WreckHunter2000/1.0 (wreck-detection research)'}

# ─── USGS products ────────────────────────────────────────────────────────────

USGS_BASE = 'https://mrdata.usgs.gov/magnetic'

USGS_FILES = [
    # (filename, description, priority)
    ('NAmag_origmrg.zip',  'N.America original merged grid — ~1km res, ~315 MB', 1),
    ('NAmag_hp500.zip',    'N.America high-pass 500km — local anomalies only, ~317 MB', 1),
    ('USmag_origmrg.zip',  'Contiguous US original merged grid — ~46 MB', 2),
    ('USmag_hp500.zip',    'Contiguous US high-pass 500km — ~47 MB', 2),
    ('magnetic.xyz.gz',    'Contiguous US XYZ text (Geographic NAD27) — 23 MB', 3),
    ('magnetic.gxf.gz',    'Contiguous US GXF grid (Albers NAD27) — 11 MB', 3),
]

# ─── NRCan Canada Open Data — Ontario Great Lakes border tiles ───────────────

NRCAN_API = 'https://open.canada.ca/data/api/3/action/package_search'

# Keywords that identify tiles covering Great Lakes border in Ontario
ONTARIO_GL_KEYWORDS = [
    'Lake Simcoe', 'Tobermory', 'Bruce', 'Kitchener', 'Sault', 'Sudbury',
    'Manitoulin', 'North Bay', 'Owen Sound', 'Windsor', 'Sarnia', 'Thunder Bay',
    'Fort Frances', 'Kenora', 'Pembroke', 'Belleville', 'Kingston', 'Niagara',
    'Hamilton', 'Barrie', 'Parry Sound', 'Georgian Bay', 'Lake Ontario',
    'Lake Erie', 'Lake Huron', 'Lake Superior',
]


def download_file(url: str, dest: Path, description: str = '') -> bool:
    """Download url to dest with progress. Skips if already complete."""
    if dest.exists() and dest.stat().st_size > 1024:
        print(f'  [skip] {dest.name} already exists ({dest.stat().st_size/1e6:.1f} MB)')
        return True

    print(f'  Downloading: {description or dest.name}')
    print(f'    URL: {url}')

    try:
        r = requests.get(url, headers=UA, stream=True, timeout=600)
        if r.status_code != 200:
            print(f'  ⚠ HTTP {r.status_code} — skipping')
            return False

        total = int(r.headers.get('content-length', 0))
        dest.parent.mkdir(parents=True, exist_ok=True)
        tmp = dest.with_suffix(dest.suffix + '.part')

        downloaded = 0
        t0 = time.time()
        with open(tmp, 'wb') as f:
            for chunk in r.iter_content(chunk_size=1 << 20):
                f.write(chunk)
                downloaded += len(chunk)
                if total:
                    pct = downloaded / total * 100
                    mb = downloaded / 1e6
                    elapsed = time.time() - t0
                    rate = mb / elapsed if elapsed > 0 else 0
                    print(f'\r    {pct:5.1f}% — {mb:.0f}/{total/1e6:.0f} MB — {rate:.1f} MB/s',
                          end='', flush=True)
        print()
        tmp.rename(dest)
        size_mb = dest.stat().st_size / 1e6
        print(f'  ✓ {dest.name} ({size_mb:.1f} MB)')
        return True

    except Exception as e:
        print(f'  ✗ Error: {e}')
        if tmp.exists():
            tmp.unlink()
        return False


def download_usgs(priority_limit: int = 2):
    """Download USGS North America aeromagnetic grids."""
    print(f'\n{"="*65}')
    print('USGS — North America Aeromagnetic Anomaly Grids')
    print(f'  Output: {OUT}/usgs/')
    print(f'{"="*65}')

    usgs_dir = OUT / 'usgs'
    usgs_dir.mkdir(exist_ok=True)

    ok = []
    for fname, desc, pri in USGS_FILES:
        if pri > priority_limit:
            print(f'  [skip priority {pri}] {fname}')
            continue
        url = f'{USGS_BASE}/{fname}'
        dest = usgs_dir / fname
        if download_file(url, dest, desc):
            ok.append(dest)

    print(f'\n  USGS: {len(ok)} files downloaded')
    return ok


def download_nrcan_ontario_tiles():
    """Download NRCan individual aeromag tiles for Ontario Great Lakes border."""
    print(f'\n{"="*65}')
    print('NRCan Canada — Ontario Aeromagnetic Survey Tiles')
    print(f'  Output: {OUT}/nrcan/')
    print(f'{"="*65}')

    nrcan_dir = OUT / 'nrcan'
    nrcan_dir.mkdir(exist_ok=True)

    # Search NRCan open data for aeromag tiles with Great Lakes keywords
    all_resources = []
    seen = set()

    for kw in ONTARIO_GL_KEYWORDS:
        try:
            r = requests.get(NRCAN_API,
                             params={'q': f'aeromagnetic "{kw}" Ontario', 'rows': 20},
                             headers=UA, timeout=30)
            if r.status_code != 200:
                continue
            results = r.json().get('result', {}).get('results', [])
            for pkg in results:
                title = pkg.get('title', '')
                pkg_id = pkg.get('id', '')
                if pkg_id in seen:
                    continue
                # Only aeromagnetic total field tiles
                if 'aeromagnetic' not in title.lower():
                    continue
                seen.add(pkg_id)
                for res in pkg.get('resources', []):
                    url = res.get('url', '')
                    # Only take direct file downloads from ftp.maps.canada.ca
                    if 'ftp.maps.canada.ca' in url and url.endswith('/'):
                        # Directory listing — enumerate
                        all_resources.append((title, url))
                        break
                    elif 'ftp.maps.canada.ca' in url:
                        all_resources.append((title, url))
                        break
        except Exception as e:
            print(f'  ⚠ NRCan search error for "{kw}": {e}')

    print(f'  Found {len(all_resources)} NRCan tile packages')

    ok = []
    for title, url in all_resources[:40]:   # cap at 40 tiles
        # For directory URLs, try to list and download .grd/.zip/.tif files
        if url.endswith('/'):
            try:
                lr = requests.get(url, headers=UA, timeout=30)
                if lr.status_code != 200:
                    continue
                files = re.findall(r'href="([^"]*\.(?:zip|grd|tif|asc|xyz))"', lr.text, re.I)
                for f in files[:3]:
                    furl = url + f if not f.startswith('http') else f
                    fname = furl.rsplit('/', 1)[-1]
                    dest = nrcan_dir / fname
                    safe_title = re.sub(r'[^a-z0-9_]', '_', title.lower())[:40]
                    dest = nrcan_dir / f'{safe_title}_{fname}'
                    if download_file(furl, dest, title):
                        ok.append(dest)
            except Exception as e:
                print(f'  ⚠ {title}: {e}')
        else:
            fname = url.rsplit('/', 1)[-1] or 'file'
            safe_title = re.sub(r'[^a-z0-9_]', '_', title.lower())[:40]
            dest = nrcan_dir / f'{safe_title}_{fname}'
            if download_file(url, dest, title):
                ok.append(dest)

    print(f'\n  NRCan: {len(ok)} files downloaded')
    return ok


def write_readme(all_files):
    """Write a README describing what was downloaded."""
    readme = OUT / 'README.txt'
    lines = [
        'CESAROPS Aeromagnetic Data Archive',
        '=' * 50,
        f'Downloaded: {time.strftime("%Y-%m-%d %H:%M UTC", time.gmtime())}',
        '',
        'SPATIAL COVERAGE: All five Great Lakes (USA + Canada)',
        'BBOX: approx. lat 41-50, lon -93 to -75',
        '',
        'SOURCE 1: USGS mrdata.usgs.gov/magnetic/',
        '  - NAmag_origmrg.zip : Original merged grid, North America, ~1km',
        '  - NAmag_hp500.zip   : High-pass 500km filtered, local anomalies best',
        '  - USmag_origmrg.zip : US subset original merged',
        '  - USmag_hp500.zip   : US subset high-pass',
        '  - magnetic.xyz.gz   : US XYZ text, Geographic NAD27',
        '  - magnetic.gxf.gz   : US GXF grid, Albers NAD27',
        '  Reference: Bankey et al. 2002, USGS OFR 2002-414',
        '  DOI: https://doi.org/10.3133/ofr02414',
        '',
        'SOURCE 2: NRCan Open Data Canada (individual Ontario tiles)',
        '  - Ontario tiles covering Great Lakes shoreline',
        '  - Aeromagnetic total field, 1:50000 survey scale',
        '  License: Open Government License - Canada',
        '',
        'NOTE ON RESOLUTION:',
        '  Satellite aeromag (SWARM, CHAMP, MF-7): ~100km — useless for wrecks',
        '  These airborne surveys: 50–400m spatial resolution (survey dependent)',
        '  Inverse-cube law: B-field from a ~10m ferrous object falls off ~r^3.',
        '  Detectable at 100–300m AGL with cesium/fluxgate sensor.',
        '  These grids are compiled at ~1km but individual surveys are much finer.',
        '',
        'FILE INVENTORY:',
    ]
    for f in sorted(all_files):
        size = f.stat().st_size if f.exists() else 0
        lines.append(f'  {f.relative_to(OUT)}  ({size/1e6:.1f} MB)')
    readme.write_text('\n'.join(lines) + '\n')
    print(f'\n  README written: {readme}')


def main():
    print('CESAROPS Aeromagnetic Great Lakes Downloader')
    print(f'Output: {OUT}')

    all_files = []

    # Priority 1+2 = NAmag (both products) + USmag subset
    all_files += download_usgs(priority_limit=3)

    # NRCan Ontario tiles
    all_files += download_nrcan_ontario_tiles()

    # Write summary
    total_mb = sum(f.stat().st_size for f in all_files if f.exists()) / 1e6
    write_readme(all_files)

    print(f'\n{"="*65}')
    print(f'AEROMAG DOWNLOAD COMPLETE')
    print(f'  Files:      {len(all_files)}')
    print(f'  Total size: {total_mb:.0f} MB')
    print(f'  Location:   {OUT}')
    print(f'{"="*65}')


if __name__ == '__main__':
    main()
