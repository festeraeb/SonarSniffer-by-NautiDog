"""
Orchestrator: select per-sensor tiles from local Bagrecovery, start immediate processing

Behavior:
- For each sensor in SENTINEL-1, SENTINEL-2, LANDSAT: pick the top 3 years by score
  (score favors thermal/night and low-water keywords) and copy up to 5 files per year
  (ensuring at least one night file per year when available). Copied into
  `data/sentinel/huron/<sensor>/<year>/`.
- Also selects 4 Icesat files total and 2 SWOT files total into `data/altimetry/`.
- Starts per-file detection on any files already present and writes per-sensor
  detection JSONs and a fused detections file.

Run from repo root: `python scripts/huron_orchestrator.py`
"""

import re
import json
import shutil
from pathlib import Path
from collections import defaultdict
import argparse
import numpy as np
from scipy import ndimage
import rasterio

ROOT = Path(__file__).resolve().parents[1]
BAGROOT_CANDIDATES = [Path.cwd().parent / 'Bagrecovery', Path('C:/Users/thomf/programming/Bagrecovery'), Path('Bagrecovery')]
DEST_ROOT = ROOT / 'data' / 'sentinel' / 'huron'
OUT = ROOT / 'outputs' / 'huron'
OUT.mkdir(parents=True, exist_ok=True)

EXTS = ('.tif', '.tiff', '.jp2', '.npy')

SENSOR_KEYWORDS = {
    'sentinel-1': ['s1', 'sentinel1', 'sentinel-1', 'vv', 'vh'],
    'sentinel-2': ['s2', 'sentinel2', 'sentinel-2', 'b01', 'b02', 'b03', 'b04'],
    'landsat': ['landsat', 'lc08', 'l8', 'l7', 'l5']
}

THERMAL_KEYWORDS = ('therm', 'tir', 'thermal', 'thermo')
NIGHT_KEYWORDS = ('night', '0200', '0300', '2300', '0000')
LOWWATER_KEYWORDS = ('lowwater', 'shore', 'shoreline', 'exposed')


def find_bagrecovery_root():
    for c in BAGROOT_CANDIDATES:
        if c.exists():
            return c
    return None


def extract_date_from_name(name):
    m = re.search(r'(20\d{2})[-_]?([01]\d)[-_]?([0-3]\d)', name)
    if m:
        return f"{m.group(1)}{m.group(2)}{m.group(3)}"
    m2 = re.search(r'(20\d{2})', name)
    if m2:
        return m2.group(1)
    return None


def is_in_excluded_winter_range(date_str):
    """Return True if date (YYYYMMDD or YYYY...) falls between Dec 15 and Mar 15 inclusive."""
    if not date_str:
        return False
    if len(date_str) < 8:
        return False
    try:
        month = int(date_str[4:6])
        day = int(date_str[6:8])
    except Exception:
        return False
    md = month * 100 + day
    if md >= 1215 or md <= 315:
        return True
    return False


def score_name(name):
    n = name.lower()
    s = 0
    if any(k in n for k in THERMAL_KEYWORDS):
        s += 100
    if any(k in n for k in NIGHT_KEYWORDS):
        s += 50
    if any(k in n for k in LOWWATER_KEYWORDS):
        s += 40
    date = extract_date_from_name(n)
    if date and len(date) >= 4:
        try:
            s += int(date[:4]) - 2000
        except Exception:
            pass
    return s


def collect_files_for_sensor(bagroot, keywords):
    out = []
    for p in bagroot.rglob('*'):
        if p.suffix.lower() in EXTS and any(k in p.name.lower() for k in keywords):
            out.append(p)
    return out


def group_by_year(files):
    years = defaultdict(list)
    for f in files:
        d = extract_date_from_name(f.name)
        if d and len(d) >= 4:
            y = d[:4]
        else:
            y = 'unknown'
        years[y].append(f)
    return years


def select_years_by_score(grouped, top_n=3):
    scores = []
    for y, files in grouped.items():
        avg = np.mean([score_name(f.name) for f in files]) if files else 0
        scores.append((avg, y))
    scores.sort(reverse=True)
    return [y for _, y in scores[:top_n]]


def select_files_for_year(files, per_year=5, night_required=1):
    # filter out files in the excluded winter window
    files = [f for f in files if not is_in_excluded_winter_range(extract_date_from_name(f.name))]
    scored = sorted(files, key=lambda f: score_name(f.name), reverse=True)
    selected = []
    night_sel = []
    for f in scored:
        if len(selected) >= per_year:
            break
        selected.append(f)
        if any(k in f.name.lower() for k in NIGHT_KEYWORDS + THERMAL_KEYWORDS):
            night_sel.append(f)
    if len(night_sel) < night_required:
        for f in scored:
            if f in selected:
                continue
            if any(k in f.name.lower() for k in NIGHT_KEYWORDS + THERMAL_KEYWORDS):
                if len(selected) < per_year:
                    selected.append(f)
                    night_sel.append(f)
            if len(night_sel) >= night_required or len(selected) >= per_year:
                break
    return selected[:per_year]


def copy_selected_to_dest(selected, sensor, year):
    dest = DEST_ROOT / sensor / year
    dest.mkdir(parents=True, exist_ok=True)
    for p in selected:
        try:
            shutil.copy2(p, dest / p.name)
        except Exception as e:
            print(f"Copy failed {p}: {e}")


def detect_on_raster(path):
    try:
        with rasterio.open(path) as src:
            arr = src.read(1).astype('float32')
            m = np.nanmean(arr)
            s = np.nanstd(arr)
            thr = m + 3.0 * s
            mask = arr > thr
            label, n = ndimage.label(mask)
            sizes = ndimage.sum(mask, label, range(1, n+1))
            good = [i+1 for i,siz in enumerate(sizes) if siz >= 10]
            dets = []
            for lab in good:
                coords = np.where(label==lab)
                r = int(np.mean(coords[0])); c = int(np.mean(coords[1]))
                cx, cy = src.transform * (c + 0.5, r + 0.5)
                dets.append({'row': r, 'col': c, 'x': float(cx), 'y': float(cy), 'size': int(np.sum(label==lab))})
            return dets
    except Exception as e:
        print('Detect failed on', path, e)
        return []


def process_and_write(sensor):
    sensor_dir = DEST_ROOT / sensor
    if not sensor_dir.exists():
        print('No files for', sensor)
        return []
    detections = []
    for year_dir in sorted(sensor_dir.iterdir()):
        if not year_dir.is_dir():
            continue
        for f in year_dir.iterdir():
            if f.suffix.lower() in EXTS:
                dets = detect_on_raster(f)
                outf = OUT / f"detections_{sensor}_{f.name}.json"
                with open(outf, 'w') as fh:
                    json.dump({'file': str(f), 'detections': dets}, fh, indent=2)
                detections.extend([{'sensor': sensor, 'x': d['x'], 'y': d['y'], 'size': d['size']} for d in dets])
    return detections


def fuse_all(all_dets):
    fused = []
    used = [False]*len(all_dets)
    for i,p in enumerate(all_dets):
        if used[i]:
            continue
        group = [p]; used[i]=True
        for j,q in enumerate(all_dets):
            if used[j]:
                continue
            dx = p['x'] - q['x']; dy = p['y'] - q['y']
            if np.hypot(dx, dy) < 50:
                group.append(q); used[j]=True
        fused.append({'count': len(group), 'members': group, 'x': float(np.mean([m['x'] for m in group])), 'y': float(np.mean([m['y'] for m in group]))})
    with open(OUT / 'fused_detections.json', 'w') as fh:
        json.dump({'fused': fused, 'total_sources': len(all_dets)}, fh, indent=2)
    print('Wrote fused detections ->', OUT / 'fused_detections.json')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--per-year', type=int, default=5)
    parser.add_argument('--years', type=int, default=3)
    parser.add_argument('--night-per-year', type=int, default=1)
    args = parser.parse_args()

    bag = find_bagrecovery_root()
    if bag is None:
        print('No Bagrecovery local repo found; please mount it.')
        return

    all_detections = []
    # per-sensor selection and copy
    for sensor, kws in SENSOR_KEYWORDS.items():
        files = collect_files_for_sensor(bag, kws)
        grouped = group_by_year(files)
        years = select_years_by_score(grouped, top_n=args.years)
        for y in years:
            year_files = grouped.get(y, [])
            selected = select_files_for_year(year_files, per_year=args.per_year, night_required=args.night_per_year)
            copy_selected_to_dest(selected, sensor, y)
        # process any files already present
        dets = process_and_write(sensor)
        all_detections.extend(dets)

    # altimetry picks: icesat and swot
    # collect 4 icesat and 2 swot files total
    icesat_files = [p for p in bag.rglob('*') if p.suffix.lower() in EXTS and 'icesat' in p.name.lower()]
    swot_files = [p for p in bag.rglob('*') if p.suffix.lower() in EXTS and 'swot' in p.name.lower()]
    dest_alt = ROOT / 'data' / 'altimetry'
    dest_alt.mkdir(parents=True, exist_ok=True)
    for p in (icesat_files[:4]):
        try: shutil.copy2(p, dest_alt / p.name)
        except: pass
    for p in (swot_files[:2]):
        try: shutil.copy2(p, dest_alt / p.name)
        except: pass

    # fuse detections across sensors
    fuse_all(all_detections)


if __name__ == '__main__':
    main()
