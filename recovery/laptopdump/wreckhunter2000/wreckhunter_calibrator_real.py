import json
import os
from pathlib import Path
from datetime import datetime

import torch
import rasterio

# Step 1: Hardware & Environment Lockdown
if not torch.cuda.is_available():
    raise RuntimeError("CUDA is not available. Check driver and GPU installation.")

cuda_device = torch.device('cuda:0')
cuda_name = torch.cuda.get_device_name(0)

conda_env = os.environ.get('CONDA_DEFAULT_ENV', '<unknown>')

# Import bag_processor handshake
try:
    import bag_processor
except Exception as e:
    raise RuntimeError(f"bag_processor module is not importable: {e}")

print(f"[+] CUDA available: {cuda_name} on {cuda_device}")
print(f"[+] Conda env: {conda_env}")
print(f"[+] bag_processor module: {bag_processor.__name__}")

# Step 2: Real-File Verification
cache = Path('..') / 'Bagrecovery' / 'outputs' / 'rossa_forensic_cache'
if not cache.exists():
    raise FileNotFoundError(f"Cache directory does not exist: {cache}")

required_files = {
    'HURON_B01': cache / 'HLS.L30.T17TLC.B01.tif',
    'HURON_B04': cache / 'HLS.L30.T17TLC.B04.tif',
    'MICHIGAN_B01': cache / 'HLS.L30.T16TET.B01.tif',
    'MICHIGAN_B04': cache / 'HLS.L30.T16TET.B04.tif',
    'ERIE_B01': cache / 'HLS.L30.T17TNE.B01.tif',
    'ERIE_B04': cache / 'HLS.L30.T17TNE.B04.tif',
}

missing = [k for k, p in required_files.items() if not p.exists()]
if missing:
    raise FileNotFoundError(f"Missing required HLS files: {', '.join(missing)}")

print("[+] All required HLS files are present.")

# Step 3: CUDA Tiled Execution

def process_pair(b01_path: Path, b04_path: Path, patch_size: int = 1024):
    # Run 0.1-99.9 quantile estimation on B01
    b01_values = []
    turb_sum = 0.0
    pix_count = 0

    with rasterio.open(b01_path) as b01_src, rasterio.open(b04_path) as b04_src:
        if b01_src.width != b04_src.width or b01_src.height != b04_src.height:
            raise RuntimeError('Dimension mismatch between B01 and B04')

        h = b01_src.height
        w = b01_src.width

        for y in range(0, h, patch_size):
            for x in range(0, w, patch_size):
                h_win = min(patch_size, h - y)
                w_win = min(patch_size, w - x)

                window = rasterio.windows.Window(x, y, w_win, h_win)
                b01_np = b01_src.read(1, window=window).astype('float32')
                b04_np = b04_src.read(1, window=window).astype('float32')

                if not torch.is_tensor(b01_np):
                    b01_t = torch.from_numpy(b01_np).to(cuda_device).float()
                else:
                    b01_t = b01_np.to(cuda_device).float()

                b04_t = torch.from_numpy(b04_np).to(cuda_device).float()

                # turbidity per patch
                turb = b04_t / (b01_t + 1e-6)
                turb_sum += float(turb.sum().item())
                pix_count += int(turb.numel())

                # donate B01 values for quantile range (on CPU) as exact
                b01_values.append(b01_t.cpu().flatten())

    b01_all = torch.cat(b01_values)
    p01 = float(torch.quantile(b01_all, 0.001).item())
    p99 = float(torch.quantile(b01_all, 0.999).item())
    denom = p99 - p01 if (p99 - p01) > 1e-6 else 1e-6

    # Re-run for SNR using stretched values in tiles
    mean_col = 0.0
    var_col = 0.0
    total = 0

    with rasterio.open(b01_path) as b01_src:
        h = b01_src.height
        w = b01_src.width

        for y in range(0, h, patch_size):
            for x in range(0, w, patch_size):
                h_win = min(patch_size, h - y)
                w_win = min(patch_size, w - x)
                window = rasterio.windows.Window(x, y, w_win, h_win)
                b01_np = b01_src.read(1, window=window).astype('float32')

                b01_t = torch.from_numpy(b01_np).to(cuda_device).float()
                stretched = torch.clamp((b01_t - p01) / denom, 0.0, 1.0)

                n = stretched.numel()
                total += n

                patch_mean = float(stretched.mean().item())
                patch_var = float(stretched.var(unbiased=False).item())

                # mean and var fusion (online)
                if total == n:
                    mean_col = patch_mean
                    var_col = patch_var
                else:
                    delta = patch_mean - mean_col
                    new_total = total
                    prev_total = total - n
                    mean_col = mean_col + delta * (n / new_total)
                    var_col = ((prev_total * var_col) + (n * patch_var) + (delta * delta) * (prev_total * n / new_total)) / new_total

    snr = float(mean_col / (var_col ** 0.5 + 1e-12))
    turbidity = turb_sum / pix_count if pix_count > 0 else float('nan')

    return {
        'turbidity': turbidity,
        'snr': snr,
        'mean_b01': float(b01_all.mean().item()),
        'p01': p01,
        'p99': p99,
    }

sites = {
    'ERIE': {'B01': Path(required_files['ERIE_B01']), 'B04': Path(required_files['ERIE_B04'])},
    'MICHIGAN': {'B01': Path(required_files['MICHIGAN_B01']), 'B04': Path(required_files['MICHIGAN_B04'])},
    'HURON': {'B01': Path(required_files['HURON_B01']), 'B04': Path(required_files['HURON_B04'])},
}

out = {
    'run_timestamp': datetime.utcnow().isoformat() + 'Z',
    'cuda_device': cuda_name,
    'conda_env': conda_env,
    'results': {},
}

for lake, paths in sites.items():
    print(f"[+] Processing {lake}")
    r = process_pair(paths['B01'], paths['B04'], patch_size=1024)
    out['results'][lake] = r
    print(f"    {lake} turbidity={r['turbidity']:.6f}, snr={r['snr']:.6f}")

# Step 4: Truth Report
out_path = Path('dist') / 'WreckHunter2000_Clean' / 'system_limits_v1_REAL.json'
out_path.parent.mkdir(parents=True, exist_ok=True)
with open(out_path, 'w', encoding='utf-8') as f:
    json.dump(out, f, indent=2)

print(f"[+] Wrote {out_path}")
print(f"[+] Michigan SNR = {out['results']['MICHIGAN']['snr']:.6f}")
print(f"[+] CUDA device = {cuda_name}")
