import os
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent.parent
os.chdir(root)

# 1) create synthetic sample data
print('1) creating sample sentinel data')
subprocess.run(['python', 'scripts/create_sample_sentinel_data.py'], check=True)

# 2) run calibration pipeline
print('2) running master_calibration')
proc = subprocess.run(['python', '-u', '-m', 'bag_processor.master_calibration'], capture_output=True, text=True)
print('--- stdout ---')
print(proc.stdout)
print('--- stderr ---')
print(proc.stderr)
if proc.returncode != 0:
    raise RuntimeError(f'master_calibration exit {proc.returncode}')

# 3) confirm output
out_dir = root / 'outputs' / 'calibration'
print('3) checking output directory:', out_dir)
print('exists', out_dir.exists())
if out_dir.exists():
    for p in sorted(out_dir.glob('*')):
        print(' -', p.name, p.stat().st_size, 'bytes')
    file = out_dir / 'calibration_v1.json'
    if file.exists():
        print('calibration_v1.json exists; first line:')
        with file.open('r', encoding='utf-8') as f:
            print(f.readline().strip())
    else:
        print('calibration_v1.json missing')

print('SUCCESS: no-real-data calibration pipeline complete')
