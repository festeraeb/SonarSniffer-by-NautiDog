#!/usr/bin/env python3
"""
GPU Inference Server — CESAROPS GPU Pool Gateway

Flask server exposing local GPU for remote z-score anomaly detection.
Extends the tpu_server.py pattern on port 5002.

Endpoints:
    POST /gpu/infer   — Accept base64 image, run CuPy z-score, return anomalies
    GET  /gpu/health   — Report GPU type, VRAM, busy/idle status
    GET  /health       — Alias for /gpu/health (compatibility)

To run:
    python gpu_server.py              # default :5002
    python gpu_server.py --port 5003  # custom port
"""
from flask import Flask, request, jsonify
import base64
import threading
import time
import sys
from io import BytesIO
from PIL import Image
import numpy as np

app = Flask(__name__)

# ── GPU detection ─────────────────────────────────────────────────────────────

GPU_INFO = {"available": False, "backend": "cpu_numpy", "name": "none", "vram_mb": 0}
_cp = None

try:
    import cupy as cp
    _cp = cp
    dev = cp.cuda.Device(0)
    GPU_INFO = {
        "available": True,
        "backend": "cupy_cuda",
        "name": dev.attributes.get("DeviceName", cp.cuda.runtime.getDeviceProperties(0)["name"].decode()),
        "vram_mb": dev.mem_info[1] // (1024 * 1024),
        "compute_capability": f"{dev.compute_capability[0]}.{dev.compute_capability[1]}",
    }
except Exception:
    # CuPy not installed or no CUDA GPU — fall back to numpy
    try:
        import cupy as cp
        _cp = cp
        GPU_INFO["backend"] = "cupy_cuda"
        GPU_INFO["available"] = True
        GPU_INFO["name"] = "unknown_cuda"
    except ImportError:
        pass

# Simple busy flag (one job at a time per GPU)
_busy_lock = threading.Lock()
_busy = False
_jobs_completed = 0


# ── Core processing ──────────────────────────────────────────────────────────

def gpu_zscore_inference(image: Image.Image, threshold: float = 2.5):
    """
    CuPy z-score anomaly detection on a grayscale tile.
    Falls back to NumPy if CuPy is unavailable.
    """
    arr = np.array(image.convert("L"), dtype=np.float32)

    if _cp is not None and GPU_INFO["available"]:
        arr_gpu = _cp.asarray(arr)
        mean_val = _cp.mean(arr_gpu)
        std_val = _cp.std(arr_gpu)
        zscore = (arr_gpu - mean_val) / (std_val + 1e-6)
        anomalies = _cp.abs(zscore) > threshold
        anomaly_count = int(_cp.sum(anomalies))

        # Top 20 anomaly locations (row, col, zscore) — transfer only these to CPU
        if anomaly_count > 0:
            flat = _cp.abs(zscore).ravel()
            top_k = min(20, anomaly_count)
            top_idx = _cp.argsort(flat)[-top_k:]
            rows, cols = _cp.unravel_index(top_idx, zscore.shape)
            top_hits = [
                {"row": int(r), "col": int(c), "zscore": round(float(zscore[r, c]), 4)}
                for r, c in zip(rows.get(), cols.get())
            ]
            max_zscore = float(_cp.max(_cp.abs(zscore)))
        else:
            top_hits = []
            max_zscore = 0.0

        result = {
            "anomaly_count": anomaly_count,
            "mean": round(float(mean_val), 4),
            "std": round(float(std_val), 4),
            "max_zscore": round(max_zscore, 4),
            "top_hits": top_hits,
            "gpu_used": True,
            "backend": "cupy_cuda",
        }
        _cp.get_default_memory_pool().free_all_blocks()
        return result

    # NumPy CPU fallback
    mean_val = np.mean(arr)
    std_val = np.std(arr)
    zscore = (arr - mean_val) / (std_val + 1e-6)
    anomalies = np.abs(zscore) > threshold
    anomaly_count = int(np.sum(anomalies))

    if anomaly_count > 0:
        flat = np.abs(zscore).ravel()
        top_k = min(20, anomaly_count)
        top_idx = np.argsort(flat)[-top_k:]
        rows, cols = np.unravel_index(top_idx, zscore.shape)
        top_hits = [
            {"row": int(r), "col": int(c), "zscore": round(float(zscore[r, c]), 4)}
            for r, c in zip(rows, cols)
        ]
        max_zscore = float(np.max(np.abs(zscore)))
    else:
        top_hits = []
        max_zscore = 0.0

    return {
        "anomaly_count": anomaly_count,
        "mean": round(float(mean_val), 4),
        "std": round(float(std_val), 4),
        "max_zscore": round(max_zscore, 4),
        "top_hits": top_hits,
        "gpu_used": False,
        "backend": "cpu_numpy",
    }


# ── Routes ────────────────────────────────────────────────────────────────────

@app.route("/gpu/infer", methods=["POST"])
def gpu_infer():
    global _busy, _jobs_completed
    start = time.time()

    data = request.get_json()
    if not data or "image_base64" not in data:
        return jsonify({"error": "missing image_base64"}), 400

    with _busy_lock:
        if _busy:
            return jsonify({"error": "gpu_busy", "retry_after_s": 5}), 503
        _busy = True

    try:
        b64 = data["image_base64"]
        meta = data.get("meta", {})
        threshold = data.get("threshold", 2.5)

        img_bytes = base64.b64decode(b64)
        img = Image.open(BytesIO(img_bytes))

        result = gpu_zscore_inference(img, threshold=threshold)
        elapsed = time.time() - start

        _jobs_completed += 1
        result["meta"] = meta
        result["took_s"] = round(elapsed, 4)
        return jsonify(result)

    except Exception as e:
        return jsonify({"error": f"inference failed: {e}"}), 500
    finally:
        with _busy_lock:
            _busy = False


@app.route("/gpu/health", methods=["GET"])
@app.route("/health", methods=["GET"])
def gpu_health():
    with _busy_lock:
        busy = _busy
    info = {
        "status": "healthy",
        "gpu": GPU_INFO,
        "busy": busy,
        "jobs_completed": _jobs_completed,
        "service": "gpu_server",
        "port": request.host.split(":")[-1] if ":" in request.host else "5002",
    }
    # Live VRAM snapshot (if CuPy available)
    if _cp is not None and GPU_INFO["available"]:
        try:
            free, total = _cp.cuda.Device(0).mem_info
            info["vram_free_mb"] = free // (1024 * 1024)
            info["vram_total_mb"] = total // (1024 * 1024)
        except Exception:
            pass
    return jsonify(info)


# ── Main ──────────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    import argparse

    p = argparse.ArgumentParser(description="CESAROPS GPU Inference Server")
    p.add_argument("--port", type=int, default=5002)
    p.add_argument("--host", default="0.0.0.0")
    args = p.parse_args()

    print(f"GPU Server — GPU: {GPU_INFO['available']} ({GPU_INFO['name']}), Backend: {GPU_INFO['backend']}")
    if GPU_INFO.get("vram_mb"):
        print(f"  VRAM: {GPU_INFO['vram_mb']} MB, Compute: {GPU_INFO.get('compute_capability', '?')}")
    print(f"  Listening on {args.host}:{args.port}")
    app.run(host=args.host, port=args.port)
