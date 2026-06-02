"""
Minimal NDBC helper: fetch latest wind speed for a given buoy id and print a simple JSON.
Usage: python scripts/day0_fetch_ndbc_and_wind.py --buoy 45002
"""
import argparse
import requests
import json
import sys

def fetch_latest_observation(buoy_id: str):
    # NDBC provides a plain text latest observations file at this path
    url = f"https://www.ndbc.noaa.gov/data/realtime2/{buoy_id}.txt"
    resp = requests.get(url, timeout=20)
    resp.raise_for_status()
    lines = resp.text.splitlines()
    # header starts with # or a line of column names; find first non-comment header
    header = None
    for ln in lines:
        if ln.startswith('#'):
            continue
        header = ln.split()
        break
    if header is None:
        raise RuntimeError('Could not parse NDBC response')
    # next non-comment line after header contains data
    data_line = None
    found_header = False
    for ln in lines:
        if ln.startswith('#'):
            continue
        if not found_header:
            # this is header
            found_header = True
            continue
        data_line = ln.split()
        break
    if data_line is None:
        raise RuntimeError('No data row found')
    # try to map header to fields
    d = dict(zip(header, data_line))
    # common wind fields: WSPD (wind speed in m/s) or WSPD
    wind = None
    for key in ('WSPD','wspd','WIND','wind'):
        if key in d:
            wind = d[key]
            break
    return {
        'buoy': buoy_id,
        'wind': wind,
        'raw_header': header,
        'raw_row': data_line
    }

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--buoy', required=True, help='NDBC buoy id, e.g. 45002')
    args = p.parse_args()
    try:
        out = fetch_latest_observation(args.buoy)
        print(json.dumps(out, indent=2))
    except Exception as e:
        print('ERROR', e, file=sys.stderr)
        sys.exit(2)

if __name__ == '__main__':
    main()
