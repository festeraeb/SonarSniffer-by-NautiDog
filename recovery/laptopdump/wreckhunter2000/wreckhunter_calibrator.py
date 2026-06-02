import json
import time
import subprocess
from datetime import datetime
from pathlib import Path
import argparse
import warnings

import math
import torch
import numpy as np

try:
    import rasterio
    from rasterio.windows import Window
    _HAS_RASTERIO = True
except Exception:
    rasterio = None
    Window = None
    _HAS_RASTERIO = False


class WreckHunterCalibrator:
    def __init__(self, device: torch.device = None):
        self.device = device or (torch.device("cuda") if torch.cuda.is_available() else torch.device("cpu"))
        print(f"[*] INITIALIZING ON DEVICE: {self.device}")

        # 2026 baselines
        self.sar_cutoff_kts = 12.0
        self.turbidity_ceiling = 1.20

    def _load_raster_to_tensor(self, path: str):
        if path is None:
            return None
        p = Path(path)
        if not p.exists():
            warnings.warn(f"Raster path not found: {path} — falling back to simulated data")
            return None

        if _HAS_RASTERIO:
            with rasterio.open(str(p)) as src:
                arr = src.read(1).astype("float32")
                arr = np.nan_to_num(arr, nan=0.0, posinf=0.0, neginf=0.0)
                return torch.from_numpy(arr).to(self.device)
        else:
            warnings.warn("rasterio not available; cannot load real rasters — returning None")
            return None

    def _gpu_temp_celsius(self) -> int | None:
        try:
            out = subprocess.check_output(
                ['nvidia-smi', '--query-gpu=temperature.gpu', '--format=csv,noheader,nounits'],
                timeout=5, stderr=subprocess.DEVNULL,
            )
            return int(out.decode().strip().splitlines()[0])
        except Exception:
            return None

    _last_thermal_check: float = 0.0
    _THERMAL_INTERVAL: float = 120.0  # seconds between proactive checks

    def gpu_cooldown_check(self, label: str = '') -> None:
        """Thermal break — proactive check at most once per _THERMAL_INTERVAL.
        Hard limit (>=83°C) loops until cool; warn level (>=78°C) sleeps 3-4 s."""
        if not torch.cuda.is_available():
            return
        now = time.monotonic()
        if now - self._last_thermal_check < self._THERMAL_INTERVAL:
            return
        self._last_thermal_check = now
        torch.cuda.empty_cache()
        temp = self._gpu_temp_celsius()
        if temp is None:
            return
        if temp >= 83:
            warnings.warn(f"GPU {temp}°C — hard thermal pause {label}")
            while True:
                time.sleep(4)
                temp = self._gpu_temp_celsius()
                if temp is None or temp < 78:
                    break
        elif temp >= 78:
            warnings.warn(f"GPU {temp}°C — brief cool break {label}")
            time.sleep(3)

    def _snr_full_band(self, path: str) -> dict:
        """Load entire band to GPU in one shot, compute SNR in a single pass.
        Returns p001, p999, mean, std, snr — no cross-band math."""
        eps = 1e-6
        with rasterio.open(path) as src:
            h, w = src.height, src.width
            t = src.transform
            origin = {'col_off': t.c, 'row_off': t.f, 'res_x': t.a, 'res_y': t.e}
            arr = np.nan_to_num(src.read(1).astype('float32'), nan=0.0, posinf=0.0, neginf=0.0)

        self.gpu_cooldown_check(f'snr {Path(path).name}')
        t = torch.from_numpy(arr).to(self.device)
        p001 = float(torch.quantile(t, 0.001))
        p999 = float(torch.quantile(t, 0.999))
        denom = (p999 - p001) if (p999 - p001) > eps else eps
        stretched = torch.clamp((t - p001) / denom, 0.0, 1.0)
        mean = float(stretched.mean())
        std  = float(stretched.std())
        std  = std if std > eps else eps
        del t, stretched
        torch.cuda.empty_cache()
        return {'p001': p001, 'p999': p999, 'mean': mean, 'std': std,
                'snr': mean / std, 'origin': origin, 'shape': (h, w)}

    def dual_stream_native_snr(self, b04_path: str, b05_path: str,
                               patch_size: int = 1024) -> dict:
        """
        Native-Resolution Persistence protocol.

        1. Independent SNR for B04 (10m) and B05 (20m) — no cross-band math.
        2. Coarse-to-fine turbidity proxy: downsample B04 to 20m grid via
           patch-level averaging (real data → coarse stats), then compute
           Turbidity_Proxy = B04_20m_mean / B05_20m_mean per aligned patch.
        3. WCS anchor: bottom-left corner recorded from rasterio transform.
        4. Rossa signature: 10m B04 hard spike inside 20m B05 spectral glow.
        """
        if not _HAS_RASTERIO:
            raise RuntimeError('rasterio required for dual_stream_native_snr')
        eps = 1e-6

        print('[+] B04 native SNR pass (10m)...')
        b04_stats = self._snr_full_band(b04_path)
        print(f'    B04  snr={b04_stats["snr"]:.4f}  mean={b04_stats["mean"]:.6f}  '
              f'std={b04_stats["std"]:.6f}  shape={b04_stats["shape"]}')

        print('[+] B05 native SNR pass (20m)...')
        b05_stats = self._snr_full_band(b05_path)
        print(f'    B05  snr={b05_stats["snr"]:.4f}  mean={b05_stats["mean"]:.6f}  '
              f'std={b05_stats["std"]:.6f}  shape={b05_stats["shape"]}')

        # Coarse-to-fine turbidity: process aligned 20m patches from both bands.
        # B04 is read at 20m-equivalent windows (2x2 native pixels averaged = 1 coarse cell).
        # B05 is read at its native 20m windows.
        # Both grids are WCS-anchored — same geographic extent, B04 patch is 2x B05 patch.
        print('[+] Coarse-to-fine turbidity proxy (B04_20m / B05_20m)...')
        turb_sum, turb_n = 0.0, 0
        rossa_spikes = []   # (patch_x, patch_y, b04_spike_score, b05_glow_score)

        b04_p001, b04_p999 = b04_stats['p001'], b04_stats['p999']
        b05_p001, b05_p999 = b05_stats['p001'], b05_stats['p999']
        b04_denom = (b04_p999 - b04_p001) if (b04_p999 - b04_p001) > eps else eps
        b05_denom = (b05_p999 - b05_p001) if (b05_p999 - b05_p001) > eps else eps

        # Load both full bands to GPU at once — one read, one transfer each
        with rasterio.open(b04_path) as src4:
            b04_full = torch.from_numpy(
                np.nan_to_num(src4.read(1).astype('float32'), nan=0.0, posinf=0.0, neginf=0.0)
            ).to(self.device)
        self.gpu_cooldown_check('post-b04-load')

        with rasterio.open(b05_path) as src5:
            h5, w5 = src5.height, src5.width
            b05_full = torch.from_numpy(
                np.nan_to_num(src5.read(1).astype('float32'), nan=0.0, posinf=0.0, neginf=0.0)
            ).to(self.device)
        self.gpu_cooldown_check('post-b05-load')

        # Downsample B04 (10m) to 20m by 2x2 average — entire array at once
        h4, w4 = b04_full.shape
        h4e, w4e = (h4 // 2) * 2, (w4 // 2) * 2
        b04_coarse = (b04_full[:h4e, :w4e]
                      .reshape(h4e // 2, 2, w4e // 2, 2)
                      .mean(dim=(1, 3)))

        # Align to B05 grid (edge may differ by 1 pixel)
        ch = min(b04_coarse.shape[0], b05_full.shape[0])
        cw = min(b04_coarse.shape[1], b05_full.shape[1])
        b04_c = b04_coarse[:ch, :cw]
        b05_p = b05_full[:ch, :cw]

        turb_map = b04_c / (b05_p + eps)
        turb_sum = float(turb_map.sum().item())
        turb_n   = turb_map.numel()

        # Rossa signature: hard spike in B04_10m inside B05 glow
        b04_norm = torch.clamp((b04_full - b04_p001) / b04_denom, 0.0, 1.0)
        b05_norm = torch.clamp((b05_p    - b05_p001) / b05_denom, 0.0, 1.0)

        b04_spike_thresh = float(b04_norm.mean()) + 2.5 * float(b04_norm.std())
        b05_glow_thresh  = float(b05_norm.mean()) + 1.5 * float(b05_norm.std())

        b05_glow_mask_10m = (b05_norm > b05_glow_thresh).repeat_interleave(2, dim=0) \
                                                          .repeat_interleave(2, dim=1)
        h4a = min(b04_norm.shape[0], b05_glow_mask_10m.shape[0])
        w4a = min(b04_norm.shape[1], b05_glow_mask_10m.shape[1])

        spike_inside_glow = (
            (b04_norm[:h4a, :w4a] > b04_spike_thresh) &
            b05_glow_mask_10m[:h4a, :w4a]
        )
        n_rossa = int(spike_inside_glow.sum().item())
        if n_rossa > 0:
            rossa_spikes.append({
                'patch_x': 0, 'patch_y': 0,
                'b04_spike_score': round(b04_spike_thresh, 4),
                'b05_glow_score':  round(b05_glow_thresh, 4),
                'rossa_pixels':    n_rossa,
            })

        del b04_full, b05_full, b04_coarse, b04_c, b05_p, turb_map, b04_norm, b05_norm
        del b05_glow_mask_10m, spike_inside_glow
        torch.cuda.empty_cache()
        self.gpu_cooldown_check('post-rossa')

        mean_turbidity = turb_sum / turb_n if turb_n > 0 else 0.0

        return {
            'b04_snr':          round(b04_stats['snr'], 4),
            'b05_snr':          round(b05_stats['snr'], 4),
            'b04_mean':         round(b04_stats['mean'], 6),
            'b05_mean':         round(b05_stats['mean'], 6),
            'b04_std':          round(b04_stats['std'], 6),
            'b05_std':          round(b05_stats['std'], 6),
            'turbidity_proxy':  round(mean_turbidity, 4),
            'rossa_patches':    len(rossa_spikes),
            'rossa_total_px':   sum(r['rossa_pixels'] for r in rossa_spikes),
            'rossa_detections': rossa_spikes[:20],  # top 20 for log
            'b04_origin':       b04_stats['origin'],
            'b05_origin':       b05_stats['origin'],
        }

    def calculate_turbidity_and_snr_from_arrays(self, b01_arr, b04_arr, mask_arr=None):
        """Core computation accepting numpy arrays (or torch tensors)."""
        eps = 1e-6

        # Convert to tensors if needed and move to device
        if not isinstance(b01_arr, torch.Tensor):
            b01 = torch.from_numpy(np.asarray(b01_arr, dtype=np.float32)).to(self.device)
        else:
            b01 = b01_arr.to(self.device).float()

        if not isinstance(b04_arr, torch.Tensor):
            b04 = torch.from_numpy(np.asarray(b04_arr, dtype=np.float32)).to(self.device)
        else:
            b04 = b04_arr.to(self.device).float()

        if mask_arr is not None:
            if not isinstance(mask_arr, torch.Tensor):
                mask = torch.from_numpy(np.asarray(mask_arr, dtype=np.bool_)).to(self.device)
            else:
                mask = mask_arr.to(self.device).bool()
        else:
            mask = None

        if mask is not None:
            b01 = torch.where(mask, b01, torch.tensor(0.0, device=self.device))
            b04 = torch.where(mask, b04, torch.tensor(0.0, device=self.device))

        # A. turbidity map
        turbidity_map = b04 / (b01 + eps)
        mean_turbidity = float(torch.mean(turbidity_map).item())

        # B. robust linear stretch using tiny quantiles
        try:
            q = torch.quantile(b01, torch.tensor([0.001, 0.999], device=self.device))
            p01, p99 = float(q[0].item()), float(q[1].item())
        except RuntimeError:
            # fallback for constant arrays or very small arrays
            p01, p99 = float(torch.min(b01).item()), float(torch.max(b01).item())

        denom = (p99 - p01) if (p99 - p01) > eps else eps
        unmasked_b01 = torch.clamp((b01 - p01) / denom, 0.0, 1.0)

        # C. SNR calculation (stability: add eps to std)
        signal = float(torch.mean(unmasked_b01).item())
        noise = float(torch.std(unmasked_b01).item())
        noise = noise if noise > eps else eps
        snr = float(signal / noise)

        return mean_turbidity, snr

    def calculate_turbidity_and_snr(self, b01_path, b04_path, mask_path=None, patch_size: int = 1024):
        """
        Stream-reading calculation to cap memory. Will iterate patches and
        aggregate turbidity mean and compute SNR in two passes.
        """
        if not _HAS_RASTERIO:
            raise RuntimeError("rasterio is required for streaming Day Zero runs")

        p01_file = Path(b01_path)
        p04_file = Path(b04_path)
        if not p01_file.exists():
            raise FileNotFoundError(f"B01 raster not found: {b01_path}")
        if not p04_file.exists():
            raise FileNotFoundError(f"B04 raster not found: {b04_path}")

        try:
            return self._calculate_full(b01_path, b04_path, mask_path)
        except (MemoryError, torch.cuda.OutOfMemoryError):
            warnings.warn('Full-band GPU load OOM — falling back to 512-patch streaming')
            return self._calculate_stream_fallback(b01_path, b04_path, mask_path, 512)

    def _calculate_full(self, b01_path, b04_path, mask_path):
        """Load full bands to GPU at once — no patch loop, one transfer each."""
        eps = 1e-6
        with rasterio.open(b01_path) as src1, rasterio.open(b04_path) as src4:
            if src4.height != src1.height or src4.width != src1.width:
                raise RuntimeError('B01/B04 dimension mismatch')
            b01_np = np.nan_to_num(src1.read(1).astype('float32'), nan=0.0, posinf=0.0, neginf=0.0)
            b04_np = np.nan_to_num(src4.read(1).astype('float32'), nan=0.0, posinf=0.0, neginf=0.0)

        self.gpu_cooldown_check('pre-calculate')
        b01 = torch.from_numpy(b01_np).to(self.device)
        b04 = torch.from_numpy(b04_np).to(self.device)
        del b01_np, b04_np

        mean_turbidity = float((b04 / (b01 + eps)).mean().item())

        p01 = float(torch.quantile(b01, 0.001))
        p99 = float(torch.quantile(b01, 0.999))
        denom = (p99 - p01) if (p99 - p01) > eps else eps
        stretched = torch.clamp((b01 - p01) / denom, 0.0, 1.0)
        mean = float(stretched.mean().item())
        std  = float(stretched.std().item())
        std  = std if std > eps else eps

        del b01, b04, stretched
        torch.cuda.empty_cache()
        self.gpu_cooldown_check('post-calculate')
        return float(mean_turbidity), float(mean / std)

    def _calculate_stream_fallback(self, b01_path, b04_path, mask_path, patch_size):
        """OOM fallback: stream 512-px patches. Same logic as _calculate_full but chunked."""
        eps = 1e-6
        turb_sum, turb_n = 0.0, 0
        n, s, sq = 0, 0.0, 0.0
        samples = []
        with rasterio.open(b01_path) as src1, rasterio.open(b04_path) as src4:
            h, w = src1.height, src1.width
            for y in range(0, h, patch_size):
                for x in range(0, w, patch_size):
                    win = Window(x, y, min(patch_size, w - x), min(patch_size, h - y))
                    b01 = torch.from_numpy(np.nan_to_num(src1.read(1, window=win).astype('float32'), nan=0.0, posinf=0.0, neginf=0.0)).to(self.device)
                    b04 = torch.from_numpy(np.nan_to_num(src4.read(1, window=win).astype('float32'), nan=0.0, posinf=0.0, neginf=0.0)).to(self.device)
                    turb_sum += float((b04 / (b01 + eps)).sum().item())
                    turb_n   += b01.numel()
                    if len(samples) < 200_000:
                        samples.extend(b01.cpu().flatten().tolist()[:max(0, 200_000 - len(samples))])
        p01, p99 = np.percentile(np.asarray(samples, dtype=np.float32), [0.1, 99.9])
        denom = (p99 - p01) if (p99 - p01) > eps else eps
        with rasterio.open(b01_path) as src1:
            h, w = src1.height, src1.width
            for y in range(0, h, patch_size):
                for x in range(0, w, patch_size):
                    self.gpu_cooldown_check(f'fallback {x},{y}')
                    win = Window(x, y, min(patch_size, w - x), min(patch_size, h - y))
                    b01 = torch.from_numpy(np.nan_to_num(src1.read(1, window=win).astype('float32'), nan=0.0, posinf=0.0, neginf=0.0)).to(self.device)
                    st = torch.clamp((b01 - p01) / denom, 0.0, 1.0)
                    n += st.numel(); s += float(st.sum()); sq += float((st * st).sum())
        mean = s / n
        std  = math.sqrt(max((sq / n) - mean * mean, eps))
        return float(turb_sum / turb_n), float(mean / std)

    def evaluate_search_probability(self, wind_kts, turbidity):
        sar_conf = 1.0 if wind_kts < self.sar_cutoff_kts else 0.4
        sar_status = "LOCKED" if wind_kts <= self.sar_cutoff_kts else "MARGINAL/FAIL"
        optical_viability = "PASS" if turbidity < self.turbidity_ceiling else "BLINDED"

        return {
            "sar_confidence_multiplier": sar_conf,
            "sar_status": sar_status,
            "optical_viability": optical_viability,
            "timestamp": datetime.utcnow().replace(microsecond=0).isoformat() + "Z"
        }

    def run_calibration_cycle(self, lake_name, wind_kts, b01_path=None, b04_path=None, mask_path=None):
        print(f"\n[!] RUNNING CALIBRATION FOR: {lake_name}")
        turbidity, snr = self.calculate_turbidity_and_snr(b01_path, b04_path, mask_path)
        prob = self.evaluate_search_probability(wind_kts, turbidity)

        results = {
            "lake": lake_name,
            "metrics": {
                "turbidity_ratio": round(float(turbidity), 3),
                "b01_snr": round(float(snr), 3),
                "wind_speed_kts": wind_kts
            },
            "system_status": prob
        }

        return results


def _default_test_data():
    return [
        {"name": "ERIE", "wind": 4.0, "b01": None, "b04": None},
        {"name": "MICHIGAN", "wind": 12.1, "b01": None, "b04": None},
        {"name": "HURON", "wind": 7.0, "b01": None, "b04": None},
    ]


def discover_hls_sites(cache_root: str):
    root = Path(cache_root)
    if not root.exists():
        raise FileNotFoundError(f"Cache directory not found: {cache_root}")

    candidate_files = list(root.rglob('*.tif'))
    by_site = {
        'erie': {'B01': None, 'B04': None},
        'michigan': {'B01': None, 'B04': None},
        'huron': {'B01': None, 'B04': None},
    }

    for f in candidate_files:
        name = f.name.lower()
        for lake in by_site:
            if lake in name:
                for band in by_site[lake]:
                    if band.lower() in name and by_site[lake][band] is None:
                        by_site[lake][band] = str(f)

    missing = []
    for lake, bands in by_site.items():
        for band, path in bands.items():
            if path is None:
                missing.append(f"{lake.upper()}_{band}")
    if missing:
        raise FileNotFoundError(f"Missing HLS tif for {', '.join(missing)} in {cache_root}")

    return by_site


def main(out_path: str, cache_root: str = 'outputs/rossa_forensic_cache'):
    # FFI handshake: verify bag_processor extension is imported
    try:
        import bag_processor
        print(f"[+] bag_processor module loaded successfully: {bag_processor.__name__}")
    except Exception as exc:
        raise RuntimeError(f"FFI handshake failed: {exc}")

    site_paths = discover_hls_sites(cache_root)

    calibrator = WreckHunterCalibrator()
    test_data = [
        {"name": "ERIE", "wind": 4.0, "b01": site_paths['erie']['B01'], "b04": site_paths['erie']['B04']},
        {"name": "MICHIGAN", "wind": 12.1, "b01": site_paths['michigan']['B01'], "b04": site_paths['michigan']['B04']},
        {"name": "HURON", "wind": 7.0, "b01": site_paths['huron']['B01'], "b04": site_paths['huron']['B04']},
    ]

    master_limits = []
    for site in test_data:
        res = calibrator.run_calibration_cycle(site['name'], site['wind'], site['b01'], site['b04'])
        # 1.729 check for Michigan
        if site['name'] == 'MICHIGAN':
            mi_snr = res['metrics']['b01_snr']
            if mi_snr < 1.0:
                res['system_status']['optical_viability'] = 'OPAQUE'
                res['system_status']['limit_warning'] = 'Michigan SNR dropped below 1.0; water flagged Opaque'
                print(f"⚠️ Michigan SNR {mi_snr:.3f} < 1.0 — opaque water limit exceeded")
            else:
                print(f"[+] Michigan SNR {mi_snr:.3f}")
            if abs(mi_snr - 1.729) > 0.001:
                print(f"[i] Benchmark 1.729 -> actual {mi_snr:.3f}")
        master_limits.append(res)

    out_dir = Path(out_path).parent
    out_dir.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w") as f:
        json.dump(master_limits, f, indent=2)

    # copy to dist/WreckHunter2000_Clean
    out_clean = Path('dist/WreckHunter2000_Clean')
    out_clean.mkdir(parents=True, exist_ok=True)
    clean_path = out_clean / Path(out_path).name
    with open(clean_path, 'w') as f:
        json.dump(master_limits, f, indent=2)

    print(f"\n[+] CALIBRATION COMPLETE. {out_path} and {clean_path} generated.")


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--out", default="system_limits_v1.json", help="Output JSON file")
    args = p.parse_args()
    main(args.out)
