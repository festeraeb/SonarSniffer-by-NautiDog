"""
swot_variant_test.py

Downloads one granule of each SWOT product variant from pass 001_160
(2023-07-26) and compares SSH values at our 7 corridor anchor coordinates.

NO DB WRITES. Read-only diagnostic.

Prints:
  - Variable list for each variant (so we know what's actually in each file)
  - SSH value at each anchor coordinate per variant
  - Delta between Unsmoothed and Expert (corrections magnitude)
"""

import json
import math
import tempfile
from pathlib import Path

import numpy as np
import requests

# ── Config ────────────────────────────────────────────────────────────────────

TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/erie_remote/erie_remote_data/.earthdata_token'),
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
]

VARIANTS = {
    'Expert':      'https://archive.swot.podaac.earthdata.nasa.gov/podaac-swot-ops-cumulus-protected/SWOT_L2_LR_SSH_2.0/SWOT_L2_LR_SSH_Expert_001_160_20230726T215351_20230726T224519_PGC0_01.nc',
    'Basic':       'https://archive.swot.podaac.earthdata.nasa.gov/podaac-swot-ops-cumulus-protected/SWOT_L2_LR_SSH_2.0/SWOT_L2_LR_SSH_Basic_001_160_20230726T215351_20230726T224519_PGC0_01.nc',
    'Unsmoothed':  'https://archive.swot.podaac.earthdata.nasa.gov/podaac-swot-ops-cumulus-protected/SWOT_L2_LR_SSH_2.0/SWOT_L2_LR_SSH_Unsmoothed_001_160_20230726T215351_20230726T224518_PGC0_01.nc',
    'WindWave':    'https://archive.swot.podaac.earthdata.nasa.gov/podaac-swot-ops-cumulus-protected/SWOT_L2_LR_SSH_2.0/SWOT_L2_LR_SSH_WindWave_001_160_20230726T215351_20230726T224519_PGC0_01.nc',
}

# 7 stationary anchors from census (corridor, highest combined_score first)
ANCHORS = [
    {'id': 1, 'lat': 42.46470, 'lon': -87.10823, 'label': 'ANC-1'},
    {'id': 2, 'lat': 42.46506, 'lon': -87.08555, 'label': 'ANC-2'},
    {'id': 3, 'lat': 42.47033, 'lon': -87.09896, 'label': 'ANC-3'},
    {'id': 4, 'lat': 42.46098, 'lon': -87.09134, 'label': 'ANC-4'},
    {'id': 5, 'lat': 42.45221, 'lon': -87.07866, 'label': 'ANC-5'},
    {'id': 6, 'lat': 42.47128, 'lon': -87.08305, 'label': 'ANC-6'},
    {'id': 7, 'lat': 42.47732, 'lon': -87.10543, 'label': 'ANC-7'},
]

SNAP_RADIUS_M = 12_000.0

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


def _download(session, url, label) -> Path | None:
    print(f'  Downloading {label}...', end=' ', flush=True)
    try:
        r = session.get(url, timeout=180, stream=True)
        r.raise_for_status()
        tmp = tempfile.NamedTemporaryFile(suffix='.nc', delete=False)
        for chunk in r.iter_content(1 << 20):
            tmp.write(chunk)
        tmp.close()
        size_mb = Path(tmp.name).stat().st_size / 1e6
        print(f'{size_mb:.1f} MB OK')
        return Path(tmp.name)
    except Exception as e:
        print(f'FAILED: {e}')
        return None


def _inspect(nc_path: Path, label: str) -> dict:
    """
    Open NetCDF, print variable list, extract SSH at each anchor.
    Returns {anchor_label: {'ssh': float, 'dist_m': float}} or None if no hit.
    """
    results = {}

    # try netCDF4 first, h5py fallback
    try:
        import netCDF4 as nc
        with nc.Dataset(str(nc_path), 'r') as ds:
            print(f'\n  [{label}] Variables:')
            for vname, var in ds.variables.items():
                shape = var.shape
                units = getattr(var, 'units', '')
                long  = getattr(var, 'long_name', '')
                print(f'    {vname:<30} {str(shape):<20} {units:<12} {long[:40]}')

            lons = np.array(ds.variables.get('longitude',
                            ds.variables.get('lon', None))[:]).flatten()
            lats = np.array(ds.variables.get('latitude',
                            ds.variables.get('lat', None))[:]).flatten()

            # SSH variable priority: ssha > ssh_karin_2 > ssh_karin
            ssh_var = None
            for candidate in ('ssha', 'ssh_karin_2', 'ssh_karin', 'ssh'):
                if candidate in ds.variables:
                    ssh_var = candidate
                    break

            if ssh_var is None:
                print(f'  [{label}] No SSH variable found')
                return results

            ssha = np.ma.filled(ds.variables[ssh_var][:].flatten(), np.nan)
            print(f'\n  [{label}] SSH variable used: {ssh_var}')
            print(f'  [{label}] Track points: {len(lons)}  '
                  f'lat range: {float(np.nanmin(lats)):.2f}–{float(np.nanmax(lats)):.2f}  '
                  f'lon range: {float(np.nanmin(lons)):.2f}–{float(np.nanmax(lons)):.2f}')

            # quality flag
            if 'quality_flag' in ds.variables:
                qf   = np.array(ds.variables['quality_flag'][:]).flatten()
                good = (qf == 0)
                print(f'  [{label}] Good quality points: {int(good.sum())} / {len(good)}')
            else:
                good = np.ones(len(lons), dtype=bool)

            for anc in ANCHORS:
                dists = np.array([_haversine_m(anc['lat'], anc['lon'],
                                               float(lats[i]), float(lons[i]))
                                  for i in range(len(lats))])
                idx = int(np.argmin(dists))
                dist = float(dists[idx])
                if dist > SNAP_RADIUS_M:
                    results[anc['label']] = None
                    continue
                val = float(ssha[idx]) if good[idx] and not np.isnan(ssha[idx]) else None
                results[anc['label']] = {'ssh_m': val, 'dist_m': round(dist, 0)}

    except ImportError:
        import h5py
        with h5py.File(str(nc_path), 'r') as f:
            print(f'\n  [{label}] HDF5 keys: {list(f.keys())}')
            def _get(keys):
                for k in keys:
                    if k in f: return np.array(f[k]).flatten()
                return None
            lons = _get(['longitude','lon'])
            lats = _get(['latitude','lat'])
            ssha = _get(['ssha','ssh_karin_2','ssh_karin','ssh'])
            if lons is None or ssha is None:
                return results
            for anc in ANCHORS:
                dists = np.array([_haversine_m(anc['lat'], anc['lon'],
                                               float(lats[i]), float(lons[i]))
                                  for i in range(len(lats))])
                idx = int(np.argmin(dists))
                dist = float(dists[idx])
                val = float(ssha[idx]) if dist <= SNAP_RADIUS_M and not np.isnan(float(ssha[idx])) else None
                results[anc['label']] = {'ssh_m': val, 'dist_m': round(dist, 0)} if val is not None else None

    return results

# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    token   = _load_token()
    session = requests.Session()
    if token:
        session.headers['Authorization'] = f'Bearer {token}'
    session.headers['User-Agent'] = 'WreckHunter2000/1.0'
    print(f'Auth token: {"loaded" if token else "MISSING"}')

    tmp_files = {}
    print('\n=== DOWNLOADING ONE OF EACH VARIANT (pass 001_160, 2023-07-26) ===\n')
    for label, url in VARIANTS.items():
        p = _download(session, url, label)
        if p:
            tmp_files[label] = p

    if not tmp_files:
        print('[!] All downloads failed.')
        return

    # Inspect each
    all_results = {}
    print('\n=== VARIABLE INSPECTION + SSH EXTRACTION ===')
    for label, nc_path in tmp_files.items():
        all_results[label] = _inspect(nc_path, label)
        nc_path.unlink(missing_ok=True)

    # Side-by-side comparison table
    sep = '=' * 78
    print(f'\n\n{sep}')
    print('SIDE-BY-SIDE SSH COMPARISON — pass 001_160  2023-07-26')
    print(f'{"Anchor":<8}', end='')
    for label in VARIANTS:
        print(f'  {label:<18}', end='')
    print()
    print('-' * 78)

    for anc in ANCHORS:
        al = anc['label']
        print(f'{al:<8}', end='')
        row_vals = {}
        for label in VARIANTS:
            hit = all_results.get(label, {}).get(al)
            if hit and hit['ssh_m'] is not None:
                val_str = f'{hit["ssh_m"]*100:+.2f}cm ({hit["dist_m"]:.0f}m)'
                row_vals[label] = hit['ssh_m']
            else:
                val_str = 'NO HIT'
                row_vals[label] = None
            print(f'  {val_str:<18}', end='')
        print()

        # delta: Unsmoothed minus Expert = magnitude of corrections
        u = row_vals.get('Unsmoothed')
        e = row_vals.get('Expert')
        if u is not None and e is not None:
            delta_cm = (u - e) * 100
            print(f'{"":8}  delta Unsmoothed-Expert: {delta_cm:+.2f} cm')

    print(sep)
    print('\nINTERPRETATION GUIDE:')
    print('  Expert     = all corrections applied (tides, DAC, wet tropo, sea state bias)')
    print('  Basic      = minimal corrections (dry tropo + solid earth tide only)')
    print('  Unsmoothed = raw KaRIn radar returns, no along-track smoothing')
    print('  WindWave   = wind/wave corrections applied, no other geophysical corrections')
    print()
    print('  For MASS DISPLACEMENT detection:')
    print('  - Unsmoothed preserves local anomalies that corrections may flatten')
    print('  - Expert removes tidal/atmospheric noise — cleaner background')
    print('  - delta(Unsmoothed - Expert) > 2cm at a coordinate = correction is')
    print('    masking a real local signal — flag for manual review')
    print()

    # Save raw comparison JSON
    out = Path('outputs/calibration/swot_variant_comparison.json')
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out, 'w') as f:
        json.dump({'pass': '001_160', 'date': '2023-07-26',
                   'results': {v: {k: (r if r else None)
                                   for k, r in res.items()}
                               for v, res in all_results.items()}}, f, indent=2)
    print(f'[+] Raw comparison written to {out}')


if __name__ == '__main__':
    main()
