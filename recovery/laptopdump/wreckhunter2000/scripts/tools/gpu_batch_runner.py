import os
import time
import rasterio
import pandas as pd
from tools.coord_sync import UniversalCoordSync
from wreckhunter2000.scripts.tools.cuda_env import configure_cuda_environment


def _init_gpu():
    configure_cuda_environment()
    import cupy as cp
    return cp

def _load_tile_to_gpu(tiff_path, band_index=1):
    """Load a raster band into GPU memory as float32."""
    cp = _init_gpu()
    with rasterio.open(tiff_path) as src:
        arr = src.read(band_index).astype('float32')
    return cp.array(arr)

def run_gpu_audit(tiff_path, z_threshold=40.0):
    """
    Backwards-compatible single-profile GPU audit.
    """
    cp = _init_gpu()
    sync = UniversalCoordSync(tiff_path)
    data_gpu = _load_tile_to_gpu(tiff_path)

    mean = cp.mean(data_gpu)
    std = cp.std(data_gpu)
    z_map = (data_gpu - mean) / std

    indices = cp.argwhere(z_map > z_threshold)
    hits = indices.get()
    z_scores = z_map[indices[:, 0], indices[:, 1]].get()

    anomalies = []
    for i, (py, px) in enumerate(hits):
        lat, lon = sync.get_geo_coords(px, py)
        anomalies.append({
            "lat": lat, "lon": lon, "z": round(float(z_scores[i]), 2),
            "tile": os.path.basename(tiff_path),
            "type": "HARD_GLINT_SINGLE"
        })

    return anomalies

def run_multi_profile_audit(tiff_path, cooldown_sec=3):
    """
    Dual-Profile Audit for the Quadro M2200.

    Profile A: Hard Glint/Metallic (Z > 12.0)
    Profile B: Thermal/Deep Sink (Z between 1.2 and 5.0)
    """
    cp = _init_gpu()
    sync = UniversalCoordSync(tiff_path)
    data_gpu = _load_tile_to_gpu(tiff_path)

    # 1. Calculate Z-Map on GPU
    z_map = (data_gpu - cp.mean(data_gpu)) / cp.std(data_gpu)

    # 2. Extract Hard Targets (Glint/Vessels)
    hard_idx = cp.argwhere(z_map > 12.0)
    hard_hits = hard_idx.get()

    # 3. Extract Subtle Targets (Heat/Deep Sinks)
    subtle_idx = cp.argwhere((z_map >= 1.2) & (z_map <= 5.0))
    subtle_hits = subtle_idx.get()

    results = []
    for py, px in hard_hits:
        lat, lon = sync.get_geo_coords(int(px), int(py))
        results.append({
            "lat": lat, "lon": lon,
            "z": float(z_map[int(py), int(px)].get()),
            "type": "HARD_GLINT",
            "tile": os.path.basename(tiff_path)
        })

    for py, px in subtle_hits:
        lat, lon = sync.get_geo_coords(int(px), int(py))
        results.append({
            "lat": lat, "lon": lon,
            "z": float(z_map[int(py), int(px)].get()),
            "type": "THERMAL_SINK",
            "tile": os.path.basename(tiff_path)
        })

    # Mandatory hardware cool-down
    time.sleep(max(0, float(cooldown_sec)))

    return results

def batch_process_corridor(folder_path, mode='multi', cooldown_sec=3, out_csv=None):
    """Batch-process all tif slides in folder_path using requested mode.

    mode: 'multi' or 'single'
    """
    all_results = []
    tiffs = [f for f in os.listdir(folder_path) if f.lower().endswith('.tif')]

    print(f"Starting GPU Audit on {len(tiffs)} slides (mode={mode})...")
    for tiff in sorted(tiffs):
        path = os.path.join(folder_path, tiff)
        try:
            if mode == 'multi':
                results = run_multi_profile_audit(path, cooldown_sec=cooldown_sec)
            else:
                results = run_gpu_audit(path)
            all_results.extend(results)
            print(f"Finished {tiff}: Found {len(results)} anomalies.")
        except Exception as e:
            print(f"Error processing {tiff}: {e}")

    # Save the Geo-Manifest
    out_csv = out_csv or f"mke_mack_cuda_manifest_{mode}.csv"
    if all_results:
        pd.DataFrame(all_results).to_csv(out_csv, index=False)
        print(f"Batch Complete. Manifest saved to {out_csv}.")
    else:
        print("Batch Complete. No anomalies detected; no manifest written.")

if __name__ == "__main__":
    batch_process_corridor("./data/mke_mack_slides", mode='multi', cooldown_sec=3)
