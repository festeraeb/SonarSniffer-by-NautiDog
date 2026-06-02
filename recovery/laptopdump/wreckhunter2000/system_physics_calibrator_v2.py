import os
from pathlib import Path
import json
import torch

from data_fetcher_scavenger import ensure_tiles
from wreckhunter_calibrator import WreckHunterCalibrator

if not torch.cuda.is_available():
    raise RuntimeError('CUDA is required for system_physics_calibrator_v2. Driver may be missing.')

print('[+] CUDA is available:', torch.cuda.get_device_name(0))
print('[+] Conda env:', os.environ.get('CONDA_DEFAULT_ENV', '<unknown>'))

try:
    import bag_processor
    print('[+] bag_processor is importable:', bag_processor.__name__)
except Exception as e:
    raise RuntimeError('bag_processor module is not importable: ' + str(e))

# Step 1: ensure rossa window tiles exist
print('[+] Ensuring Rossa window tiles are present...')
file_map = ensure_tiles('rossa')

# Step 2: file size and path logging
for lake, bands in file_map.items():
    print(f'\n[+][{lake}] file verification:')
    for band, path in bands.items():
        size = Path(path).stat().st_size
        print(f'    {band}: {path} ({size} bytes)')

# Step 3: calibrate
calibrator = WreckHunterCalibrator()
results = { 'run_timestamp': torch.datetime if False else '', 'cuda_device': torch.cuda.get_device_name(0), 'sites': [] }

lake_wind = {'Erie': 4.0, 'Michigan': 12.1, 'Huron': 7.0}

for lake in ['Erie', 'Michigan', 'Huron']:
    b01 = str(file_map[lake]['B01'])
    b04 = str(file_map[lake]['B04'])
    print(f'\n[+] Running calibration for {lake}')
    r = calibrator.run_calibration_cycle(lake, lake_wind[lake], b01, b04)
    # forensic comparator
    if lake == 'Michigan':
        snr = r['metrics']['b01_snr']
        if snr < 1.0:
            r['system_status']['optical_viability'] = 'OPAQUE'
            r['system_status']['limit_warning'] = 'Michigan SNR below 1.0 threshold'
        print(f'[!] Michigan SNR = {snr:.3f}')
    results['sites'].append(r)

# Step 4: fetch 2024 baseline (Birth of a Ghost) after processing 2025
print('\n[+] Fetching baseline 2024 tiles...')
baseline_map = ensure_tiles('baseline')
print('[+] Baseline tiles fetched. Not computing SNR until user confirms.')

out_dir = Path('dist') / 'WreckHunter2000_Clean'
out_dir.mkdir(parents=True, exist_ok=True)
out_file = out_dir / 'system_limits_v1_REAL.json'

with open(out_file, 'w', encoding='utf-8') as f:
    json.dump(results, f, indent=2)

print(f'\n[+] Generated {out_file}')
print('[+] CUDA device used:', torch.cuda.get_device_name(0))
