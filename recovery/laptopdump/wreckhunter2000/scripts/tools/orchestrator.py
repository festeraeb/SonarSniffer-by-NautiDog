import os
import json
import pandas as pd
import rasterio
from pathlib import Path
from datetime import datetime
from pyproj import Transformer
import simplekml

from .buoy_weather_checker import select_glint_windows
from .metadata_parser import extract_sentinel_metadata
from wreckhunter2000.data_fetcher_scavenger import ensure_tiles, TARGET_SCENES
from .cuda_env import configure_cuda_environment



def run_system_operator(year: int, tile: str, station_ids=None, num_days=8):
    if station_ids is None:
        station_ids = ['45002', '45007', '45161']

    #1. Environmental Filter
    glint_days = []
    for station in station_ids:
        glint_days.extend(select_glint_windows(station, year, num_days))
        if len(glint_days) >= num_days:
            break
    glint_days = sorted(set(glint_days))[:num_days]
    print('Selected high-quality glint windows:', glint_days)

    # 2. Precision Ingest
    scene = next((s for s in TARGET_SCENES if s['mgrs_tile'] == tile), None)
    if scene is None:
        raise ValueError(f'Tile {tile} not configured')

    print('Downloading Sentinel tile(s) for', tile)
    results = ensure_tiles(year_window='rossa', scene_list=[scene])
    manifest = []

    for band, path in results[scene['lake']].items():
        xml_path = Path(path).with_name('MTD_MSIL2A.xml')
        if xml_path.exists():
            meta = extract_sentinel_metadata(xml_path)
            manifest.append({
                'date': ','.join(d.strftime('%Y-%m-%d') for d in glint_days),
                'tile': tile,
                'band': band,
                'path': str(path),
                'ulx': meta['origin_x'],
                'uly': meta['origin_y'],
                'crs': meta['crs'],
            })

    df = pd.DataFrame(manifest)
    df.to_csv('glint_aoi_manifest.csv', index=False)
    print('Saved manifest glint_aoi_manifest.csv')

    # 3. Mass Audit
    os.system('python hard_pixel_audit.py')

    # 4. Multi-Pass Validation
    os.system('python triple_lock_fusion.py')

    # 5. Shape Extraction
    os.system('python gpu_curvelets.py')

    print('Orchestration complete for year', year)


def _get_slide_date_from_name(filename: str):
    """Extract date from typical filename patterns like S2C..._YYYYMMDD_..."""
    import re
    m = re.search(r'(20\d{6})', filename)
    if m:
        try:
            return datetime.strptime(m.group(1), '%Y%m%d').date()
        except ValueError:
            pass
    return None


def _slide_center_latlon(tiff_path: str):
    with rasterio.open(tiff_path) as src:
        bounds = src.bounds
        center_x = (bounds.left + bounds.right) / 2.0
        center_y = (bounds.bottom + bounds.top) / 2.0
        transformer = Transformer.from_crs(src.crs, 'EPSG:4326', always_xy=True)
        lon, lat = transformer.transform(center_x, center_y)
    return float(lat), float(lon)


def run_milwaukee_to_mackinac_mission(scan_dir: str, output_dir: str = 'outputs', cooldown_sec: int = 3):
    """Mission Orchestrator - dual-profile audit + set of mission control mandates."""
    configure_cuda_environment()

    os.makedirs(output_dir, exist_ok=True)
    slides = []
    for ext in ('*.tif', '*.TIF', '*.tiff', '*.TIFF'):
        slides += list(Path(scan_dir).rglob(ext))

    # filter by geographic latitude range
    candidate_slides = []
    for slide in sorted(slides):
        lat, lon = _slide_center_latlon(str(slide))
        if 43.0 <= lat <= 46.0:
            candidate_slides.append({'path': str(slide), 'lat': lat, 'lon': lon, 'date': _get_slide_date_from_name(slide.name)})

    if len(candidate_slides) < 20:
        print(f'[WARN] only {len(candidate_slides)} slides found in lat range 43-46; attempting data fetcher finalization')
        scene_list = TARGET_SCENES
        ensure_tiles(year_window='rossa', scene_list=scene_list)
        # re-scan after fetch
        return run_milwaukee_to_mackinac_mission(scan_dir, output_dir, cooldown_sec)

    print(f'[INFO] Data inventory: {len(candidate_slides)} eligible slides')

    # 2. Environmental pre-screen
    for s in candidate_slides:
        if s['date'] is not None:
            window = select_glint_windows('45002', s['date'].year, max_days=50)
            s['weather_tag'] = 'Prime Glint' if s['date'] in window else 'Standard'
        else:
            s['weather_tag'] = 'Unknown'

    from wreckhunter2000.scripts.tools.gpu_batch_runner import run_multi_profile_audit

    all_hits = []
    mission_log = []

    for idx, s in enumerate(candidate_slides, 1):
        print(f"[SCAN {idx}/{len(candidate_slides)}] {s['path']} ({s['weather_tag']})")
        hits = run_multi_profile_audit(s['path'], cooldown_sec=cooldown_sec)
        mission_log.append({
            'slide': s['path'],
            'date': str(s['date']),
            'weather_tag': s['weather_tag'],
            'hit_count': len(hits),
        })

        for h in hits:
            h.update({'slide': s['path'], 'date': str(s['date']), 'weather_tag': s['weather_tag']})
            all_hits.append(h)

    # 4. Triple-lock confirm across dates
    from collections import defaultdict

    bin_counter = defaultdict(list)
    for h in all_hits:
        key = (round(h['lat'], 5), round(h['lon'], 5), h['type'])
        bin_counter[key].append(h)

    confirmed = []
    for key, hits in bin_counter.items():
        dates = sorted({h['date'] for h in hits if h['date']})
        if len(dates) >= 3:
            confirmed.append({'lat': key[0], 'lon': key[1], 'type': key[2], 'dates': dates, 'count': len(hits)})

    # 4a. call specialized tools for confirmed targets
    for c in confirmed:
        if c['type'] == 'THERMAL_SINK':
            os.system('python hard_pixel_audit.py')
            mission_log.append({'action': 'hard_pixel_audit', 'target': c, 'reason': 'confirmed thermal sink'})
        elif c['type'] == 'HARD_GLINT':
            os.system('python gpu_curvelets.py')
            mission_log.append({'action': 'gpu_curvelets', 'target': c, 'reason': 'confirmed hard glint'})

    # 5. Output & reporting (simple kml)
    kml = simplekml.Kml()
    for h in all_hits:
        p = kml.newpoint(name=h['type'], coords=[(h['lon'], h['lat'])])
        p.description = f"z={h['z']:.2f}, file={h['slide']}, weather_tag={h['weather_tag']}"

    out_kml = Path(output_dir) / 'milwaukee_mackinac_scan.kml'
    kml.save(str(out_kml))

    # mission logic JSON
    with open(Path(output_dir) / 'mission_logic.json', 'w', encoding='utf-8') as f:
        json.dump({'mission_log': mission_log, 'confirmed': confirmed, 'hits': all_hits}, f, indent=2)

    print(f'[RESULT] {len(all_hits)} hits, {len(confirmed)} confirmed across 3+ dates')
    print(f'[OUTPUT] kml={out_kml}, mission_logic.json')


if __name__ == '__main__':
    # Default behavior: run the Milwaukee-to-Mackinac deep run mission
    base_scan_dir = r"C:\Users\thomf\programming\wreckhunter2000\data\cache\census_raw"
    run_milwaukee_to_mackinac_mission(scan_dir=base_scan_dir, output_dir='outputs', cooldown_sec=3)
