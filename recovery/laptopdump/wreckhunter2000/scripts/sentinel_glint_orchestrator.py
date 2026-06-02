import argparse
import datetime
import os
import sys
from pathlib import Path

import pandas as pd
import xml.etree.ElementTree as ET
from pyproj import CRS, Transformer

try:
    from seebuoy import NDBC
except ImportError:
    NDBC = None

from wreckhunter2000.data_fetcher_scavenger import ensure_tiles, TARGET_SCENES


def extract_sentinel_metadata(xml_path):
    tree = ET.parse(xml_path)
    root = tree.getroot()

    projection = root.find('.//HORIZ_CS_CODE').text

    ulx_elem = root.find(".//Geocoding[@resolution='10']//ULX")
    uly_elem = root.find(".//Geocoding[@resolution='10']//ULY")
    if ulx_elem is None or uly_elem is None:
        raise RuntimeError('Could not locate ULX/ULY for 10m resolution in XML')

    ulx = float(ulx_elem.text)
    uly = float(uly_elem.text)

    crs = CRS.from_string(projection)

    affine_matrix = {
        'origin_x': ulx,
        'origin_y': uly,
        'pixel_width': 10.0,
        'pixel_height': -10.0,
        'crs': projection,
        'crs_obj': crs,
    }
    return affine_matrix


def is_ideal_glint_day(station_id, date):
    if NDBC is None:
        raise RuntimeError('seebuoy is required for NDBC API. pip install seebuoy')

    ndbc = NDBC()
    df = ndbc.get_station(station_id)

    if isinstance(date, str):
        date = pd.to_datetime(date).date()

    if date not in df.index:
        return False

    daily_avg = df.loc[date].mean()
    if daily_avg.get('WSPD', 999) < 6.0 and daily_avg.get('WVHT', 999) < 0.5:
        return True
    return False


def find_ideal_dates(station_id, year, n_days=8, start='05-20', end='10-15'):
    if NDBC is None:
        raise RuntimeError('seebuoy is required for NDBC API. pip install seebuoy')

    ndbc = NDBC()
    df = ndbc.get_station(station_id)
    df.index = pd.to_datetime(df.index)

    start_date = datetime.date(year, int(start.split('-')[0]), int(start.split('-')[1]))
    end_date = datetime.date(year, int(end.split('-')[0]), int(end.split('-')[1]))

    candidate_days = []
    for d in pd.date_range(start=start_date, end=end_date, freq='D'):
        if d.date() in df.index.date and is_ideal_glint_day(station_id, d.date()):
            candidate_days.append(d.date())

    return candidate_days[:n_days]


def main():
    parser = argparse.ArgumentParser(description='Sentinel glint-selection scan orchestrator')
    parser.add_argument('--station', default='45002', help='NDBC station ID')
    parser.add_argument('--year', type=int, default=2025)
    parser.add_argument('--tile', default='16TDN', help='MGRS tile')
    parser.add_argument('--ndays', type=int, default=8)
    parser.add_argument('--manifest', default='glint_master_manifest.csv')
    parser.add_argument('--require-gpu', action='store_true')
    args = parser.parse_args()

    ideal_dates = find_ideal_dates(args.station, args.year, n_days=args.ndays)
    print(f'Ideal glint dates for station {args.station} in {args.year}: {ideal_dates}')

    rows = []

    for date in ideal_dates:
        scene = next((s for s in TARGET_SCENES if s['mgrs_tile'] == args.tile), None)
        if scene is None:
            raise RuntimeError(f'Unknown tile {args.tile} in TARGET_SCENES')

        print(f'Processing candidate date {date} / tile {args.tile}...')

        downloaded = ensure_tiles(scene_list=[scene], year_window='rossa')

        manifest_date = date.strftime('%Y%m%d')
        for band, path in downloaded[scene['lake']].items():
            xml_path = Path(path).with_name('MTD_MSIL2A.xml')
            if xml_path.exists():
                meta = extract_sentinel_metadata(xml_path)
                rows.append({
                    'date': manifest_date,
                    'tile': args.tile,
                    'band': band,
                    'path': str(path),
                    'ulx': meta['origin_x'],
                    'uly': meta['origin_y'],
                    'crs': meta['crs'],
                })
            else:
                rows.append({
                    'date': manifest_date,
                    'tile': args.tile,
                    'band': band,
                    'path': str(path),
                    'ulx': None,
                    'uly': None,
                    'crs': None,
                })

    df_manifest = pd.DataFrame(rows)
    df_manifest.to_csv(args.manifest, index=False)
    print(f'Wrote master manifest to {args.manifest}')

    print('---')
    print('Running hard_pixel_audit for each ideal date')
    for date in ideal_dates:
        print('Running hard_pixel_audit for', date)
        os.system('python hard_pixel_audit.py')

    print('---')
    print('Running triple lock and curvelet finalization')
    os.system('python triple_lock_fusion.py')
    os.system('python full_basin_scan.py')


if __name__ == '__main__':
    sys.exit(main())
