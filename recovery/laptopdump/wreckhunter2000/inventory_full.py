from pathlib import Path
import sqlite3

repo = Path('c:/Users/thomf/programming/wreckhunter2000')
bag  = Path('c:/Users/thomf/programming/Bagrecovery')

print('=== PIPELINE MODULES ===')
modules = [
    ('census_db',        repo / 'LAKE_MICHIGAN_CENSUS_2026.db'),
    ('swot_layer',       repo / 'swot_displacement_layer.py'),
    ('swot_extractor',   repo / 'swot_ssh_extractor.py'),
    ('lake_census',      repo / 'lake_census_engine.py'),
    ('data_fetcher',     repo / 'data_fetcher_scavenger.py'),
    ('calibrator',       repo / 'wreckhunter_calibrator.py'),
    ('sandbox_exe',      repo / 'sandbox_app/target/debug/sandbox_app.exe'),
    ('epoch_2024',       bag  / 'outputs/rossa_baseline_202408/optical_all_concepts.json'),
    ('epoch_2025',       bag  / 'outputs/rossa_forensic_202509/optical_all_concepts.json'),
    ('day0_cal',         repo / 'wreck_hunting_ml/scripts/day_zero_calibration_strict.py'),
    ('shipwrecks_db',    repo / 'databases/shipwrecks.db'),
    ('wh2k_db',          repo / 'databases/wh2k.db'),
    ('advanced_scanner', repo / 'bag_processor/advanced_bag_scanner.py'),
    ('bag_detector',     repo / 'bag_processor/bag_wreck_detector.py'),
]
for name, path in modules:
    ok   = path.exists()
    size = f'{path.stat().st_size/1e6:.2f}MB' if ok else ''
    print(f'  {"OK" if ok else "MISSING":7}  {name:28}  {size}')

print()
print('=== NPY BANDS ON DISK ===')
npy = (list((bag / 'outputs').rglob('*.npy')) +
       list(repo.rglob('sentinel_bands/*.npy')) +
       list(repo.rglob('*.npy')))
npy = [f for f in npy if 'target' not in str(f)]
for f in sorted(npy)[:40]:
    print(f'  {f.parent.name}/{f.name}  {f.stat().st_size/1e6:.1f}MB')
if not npy:
    print('  none found')

print()
print('=== TIF/COG FILES ON DISK ===')
tifs = (list((bag / 'outputs').rglob('*.tif')) +
        list(repo.rglob('*.tif')))
tifs = [f for f in tifs if 'target' not in str(f)]
for f in sorted(tifs)[:20]:
    print(f'  {f.parent.name}/{f.name}  {f.stat().st_size/1e6:.1f}MB')
if not tifs:
    print('  none found')

print()
print('=== DATABASES ===')
for db_path in [repo / 'databases/shipwrecks.db',
                repo / 'databases/wh2k.db',
                repo / 'LAKE_MICHIGAN_CENSUS_2026.db']:
    if db_path.exists():
        conn = sqlite3.connect(str(db_path))
        cur  = conn.cursor()
        cur.execute("SELECT name FROM sqlite_master WHERE type='table'")
        tables = cur.fetchall()
        print(f'  {db_path.name}:')
        for (tbl,) in tables:
            cur.execute(f'SELECT COUNT(*) FROM "{tbl}"')
            print(f'    {tbl}: {cur.fetchone()[0]} rows')
        conn.close()
    else:
        print(f'  MISSING: {db_path.name}')

print()
print('=== BAG FILES ===')
bags = list(repo.rglob('*.bag'))
for f in bags[:10]:
    print(f'  {f.name}  {f.stat().st_size/1e6:.1f}MB')
if not bags:
    print('  none found')
