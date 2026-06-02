import shutil
from pathlib import Path
from datetime import datetime, timezone

src = Path('c:/Users/thomf/programming/wreckhunter2000')
dst = src / 'dist' / 'WreckHunter2000_Clean'
dst.mkdir(parents=True, exist_ok=True)

files = [
    'lake_census_engine.py',
    'swot_displacement_layer.py',
    'swot_ssh_extractor.py',
    'swot_variant_test.py',
    'swot_shape_probe.py',
    'wreckhunter_calibrator.py',
    'data_fetcher_scavenger.py',
    'LAKE_MICHIGAN_CENSUS_2026.db',
    'outputs/calibration/census_bridge.json',
    'outputs/calibration/swot_displacement_report.json',
    'outputs/calibration/swot_variant_comparison.json',
]

archived, skipped = [], []
for f in files:
    s = src / f
    d = dst / f
    if s.exists():
        d.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(str(s), str(d))
        archived.append(f)
    else:
        skipped.append(f)

ts = datetime.now(timezone.utc).isoformat()
print(f'[+] Archive timestamp : {ts}')
print(f'[+] Destination       : {dst}')
print(f'[+] Archived          : {len(archived)} files')
for a in archived:
    print(f'    + {a}')
if skipped:
    print(f'[!] Skipped (not found): {len(skipped)}')
    for s in skipped:
        print(f'    - {s}')
