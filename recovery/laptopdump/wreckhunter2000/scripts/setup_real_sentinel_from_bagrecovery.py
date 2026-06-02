import os
import numpy as np
from pathlib import Path

# Reuse data from Bagrecovery sentinel_bands (clear / storm)
SOURCE_DIR = Path(r'c:/Users/thomf/programming/Bagrecovery/wreck_hunting_ml/sentinel_bands')
TARGET_ROOT = Path('data/sentinel')
TARGET = TARGET_ROOT / 'mb2'

print('source dir', SOURCE_DIR)
if not SOURCE_DIR.exists():
    raise SystemExit('Source sentinel_bands not found')

# ensure target folder structure
for subdir in ['pass_b_unmasked', 'pass_a_masked']:
    (TARGET / subdir).mkdir(parents=True, exist_ok=True)

# map B02/B03 to B01/B03 for proxy real data
b02 = np.load(SOURCE_DIR / 'clear_B02.npy') if (SOURCE_DIR / 'clear_B02.npy').exists() else None
b03 = np.load(SOURCE_DIR / 'clear_B03.npy') if (SOURCE_DIR / 'clear_B03.npy').exists() else None
if b02 is None or b03 is None:
    raise SystemExit('Missing B02/B03 in source data')

# Save as required names
for subdir in ['pass_b_unmasked', 'pass_a_masked']:
    np.save(TARGET / subdir / 'B01.npy', b02)
    np.save(TARGET / subdir / 'B03.npy', b03)

# use simple mask
mask = (b02 > np.percentile(b02, 10)) & (b02 < np.percentile(b02, 99))
np.save(TARGET / 'mask.npy', mask.astype(bool))

metadata = {
    'wind_speed_kts': 5.0,
    'turbidity_b04_b01': 1.0,
    'ecostress_night_val': 0.1,
    'ecostress_night_mean': 0.0,
    'ecostress_night_std': 0.01,
    'sar_phase_5_knots': 0.01,
    'sar_phase_15_knots': 0.03,
}

with open(TARGET / 'metadata.json', 'w', encoding='utf-8') as f:
    import json
    json.dump(metadata, f)

print('setup complete, target data path:', TARGET)
