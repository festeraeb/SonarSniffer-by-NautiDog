#!/usr/bin/env python3
"""
CPU-only stand-ins for Scout / Validator / Jitter vision workers.

Use when real GPUs are reserved for LLM research (cesarops2 lab mode).
Implements the same HTTP contracts as:
  - vision-workers/scout_1060.py   POST /analyze
  - vision-workers/validator_p1000.py POST /validate
  - jitter TPU stub                 POST /jitter

No torch — numpy + PIL heuristics only. Slow is fine for pipeline wiring tests.
"""
from __future__ import annotations

import argparse
import os
import base64
import hashlib
import io
import logging
import time
from typing import List, Optional

import numpy as np
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
import uvicorn

try:
    from PIL import Image
except ImportError:
    Image = None  # type: ignore

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
log = logging.getLogger("cpu_sim")

SIM_MODE = "cpu_heuristic"  # surfaced in /health


# ── Shared image helpers ─────────────────────────────────────────────────────

def _decode_image(image_b64: str) -> np.ndarray:
    if Image is None:
        raise HTTPException(status_code=500, detail="PIL not installed")
    try:
        raw = base64.b64decode(image_b64)
        img = Image.open(io.BytesIO(raw)).convert("L")
        return np.asarray(img, dtype=np.float32)
    except Exception as e:
        raise HTTPException(status_code=400, detail=f"Invalid image: {e}") from e


def _image_features(gray: np.ndarray) -> dict:
    """Cheap CPU features — not wreck detection, just plausible API behavior."""
    if gray.size < 16:
        return {"mean": 0.0, "std": 0.0, "edge": 0.0, "center_contrast": 0.0}

    h, w = gray.shape
    cy0, cy1 = h // 4, 3 * h // 4
    cx0, cx1 = w // 4, 3 * w // 4
    center = gray[cy0:cy1, cx0:cx1]
    surround = np.concatenate([
        gray[:cy0, :].ravel(),
        gray[cy1:, :].ravel(),
        gray[cy0:cy1, :cx0].ravel(),
        gray[cy0:cy1, cx1:].ravel(),
    ])
    center_mean = float(np.mean(center)) if center.size else 0.0
    surround_mean = float(np.mean(surround)) if surround.size else center_mean
    std = float(np.std(gray))
    # Sobel-ish edge density
    gx = np.abs(np.diff(gray, axis=1, prepend=gray[:, :1]))
    gy = np.abs(np.diff(gray, axis=0, prepend=gray[:1, :]))
    edge = float(np.mean(gx + gy))
    return {
        "mean": float(np.mean(gray)),
        "std": std,
        "edge": edge,
        "center_contrast": abs(center_mean - surround_mean),
    }


def _seed_confidence(tile_id: str, lat: float, lon: float, salt: str) -> float:
    """Deterministic 0.35–0.92 for reproducible integration tests."""
    key = f"{tile_id}:{lat:.5f}:{lon:.5f}:{salt}".encode()
    h = hashlib.sha256(key).hexdigest()
    return 0.35 + (int(h[:8], 16) % 5800) / 10000.0


# ── Scout API ─────────────────────────────────────────────────────────────────

class TilePatch(BaseModel):
    tile_id: str
    lat: float = 0.0
    lon: float = 0.0
    image_b64: str
    task: str = "anomaly_detection"


class ScoutReport(BaseModel):
    tile_id: str
    has_anomaly: bool
    confidence: float
    anomaly_type: str
    description: str
    bbox: Optional[List[float]] = None
    processing_ms: float


scout_app = FastAPI(title="CPU Sim Scout", version="1.0.0")


@scout_app.get("/health")
async def scout_health():
    return {"service": "scout-cpu-sim", "model": SIM_MODE, "device": "cpu", "status": "ok"}


@scout_app.post("/analyze", response_model=ScoutReport)
async def scout_analyze(req: TilePatch):
    t0 = time.time()
    gray = _decode_image(req.image_b64)
    f = _image_features(gray)
    base = _seed_confidence(req.tile_id, req.lat, req.lon, "scout")

    # High edge + center contrast → positive scout (mimics structure on water)
    signal = (f["edge"] / 40.0) + (f["center_contrast"] / 30.0)
    has_anomaly = signal > 0.35 or f["std"] > 25.0
    conf = min(0.95, base + (0.25 if has_anomaly else 0.0) + signal * 0.15)
    if has_anomaly and conf < 0.62:
        conf = 0.62  # pass Lock 1 threshold (0.6) when we flag anomaly

    atype = "linear_feature" if has_anomaly and f["edge"] > 8 else "none"
    desc = (
        f"[cpu_sim] task={req.task} edge={f['edge']:.2f} contrast={f['center_contrast']:.2f}"
        if has_anomaly
        else "[cpu_sim] no structure above threshold"
    )
    return ScoutReport(
        tile_id=req.tile_id,
        has_anomaly=has_anomaly,
        confidence=round(conf, 3),
        anomaly_type=atype,
        description=desc,
        bbox=[0.35, 0.35, 0.65, 0.65] if has_anomaly else None,
        processing_ms=(time.time() - t0) * 1000,
    )


# ── Validator API ─────────────────────────────────────────────────────────────

class ValidationRequest(BaseModel):
    tile_id: str
    lat: float = 0.0
    lon: float = 0.0
    image_b64: str


class ValidationReport(BaseModel):
    tile_id: str
    has_anomaly: bool
    confidence: float
    shape_analysis: str
    description: str
    material_guess: str
    processing_ms: float


validator_app = FastAPI(title="CPU Sim Validator", version="1.0.0")


@validator_app.get("/health")
async def validator_health():
    return {"service": "validator-cpu-sim", "model": SIM_MODE, "device": "cpu", "status": "ok"}


@validator_app.post("/validate", response_model=ValidationReport)
async def validator_validate(req: ValidationRequest):
    t0 = time.time()
    gray = _decode_image(req.image_b64)
    f = _image_features(gray)
    base = _seed_confidence(req.tile_id, req.lat, req.lon, "validator")

    has_anomaly = f["center_contrast"] > 5.0 or f["edge"] > 6.0
    conf = min(0.95, base + (0.2 if has_anomaly else 0.0))
    if has_anomaly and conf < 0.52:
        conf = 0.52

    shape = "linear" if has_anomaly and f["edge"] > 10 else ("irregular" if has_anomaly else "none")
    material = "metal" if has_anomaly and f["mean"] < 100 else ("sand" if f["std"] < 8 else "unknown")

    return ValidationReport(
        tile_id=req.tile_id,
        has_anomaly=has_anomaly,
        confidence=round(conf, 3),
        shape_analysis=shape,
        description=f"[cpu_sim] independent check contrast={f['center_contrast']:.1f}",
        material_guess=material,
        processing_ms=(time.time() - t0) * 1000,
    )


# ── Jitter API ────────────────────────────────────────────────────────────────

class JitterRequest(BaseModel):
    tile_id: str
    thermal_timeseries: List[str] = []
    coordinates: dict = {}
    depth_estimate_m: float = 150.0


class JitterSignature(BaseModel):
    material: str
    certainty: float
    depth_estimate_ft: float
    jitter_frequency_hz: float
    thermal_delta_c: float
    classification: str


jitter_app = FastAPI(title="CPU Sim Jitter", version="1.0.0")


@jitter_app.get("/health")
async def jitter_health():
    return {"service": "jitter-cpu-sim", "model": SIM_MODE, "device": "cpu", "status": "ok"}


@jitter_app.post("/jitter", response_model=JitterSignature)
async def jitter_check(req: JitterRequest):
    lat = float(req.coordinates.get("lat", 0.0))
    lon = float(req.coordinates.get("lon", 0.0))
    n_bands = len(req.thermal_timeseries)
    base = _seed_confidence(req.tile_id, lat, lon, "jitter")

    # Non-empty thermal series → higher certainty (simulates real TPU path)
    certainty = min(0.95, base + 0.15 * min(n_bands, 5))
    if certainty < 0.72 and n_bands >= 2:
        certainty = 0.72  # pass Lock 3 when bands provided

    depth_ft = req.depth_estimate_m * 3.28084
    return JitterSignature(
        material="ferrous_composite" if certainty > 0.7 else "natural",
        certainty=round(certainty, 3),
        depth_estimate_ft=round(depth_ft, 1),
        jitter_frequency_hz=round(0.02 + (base % 0.05), 4),
        thermal_delta_c=round(0.5 + base * 2.0, 2),
        classification="confirmed_structure" if certainty >= 0.7 else "likely_natural",
    )


# ── Launcher ──────────────────────────────────────────────────────────────────

# Validator on 5572 so 5571 stays free for llama-server draft (Forge/Picasso).
JITTER_PORT = int(os.environ.get("JITTER_PORT", "8080"))

ROLES = {
    "scout": (scout_app, 5570),
    "validator": (validator_app, 5572),
    "jitter": (jitter_app, JITTER_PORT),
}


def main() -> None:
    p = argparse.ArgumentParser(description="CPU simulation workers for triple-lock")
    p.add_argument(
        "role",
        choices=["scout", "validator", "jitter", "all"],
        help="Which worker to run (all = three processes)",
    )
    p.add_argument("--host", default="127.0.0.1")
    args = p.parse_args()

    if args.role == "all":
        import multiprocessing

        def _run(role: str) -> None:
            app, port = ROLES[role]
            uvicorn.run(app, host=args.host, port=port, log_level="warning")

        procs = []
        for role in ("scout", "validator", "jitter"):
            proc = multiprocessing.Process(target=_run, args=(role,), daemon=True)
            proc.start()
            procs.append(proc)
            log.info("Started %s on %s:%s", role, args.host, ROLES[role][1])
        try:
            while True:
                time.sleep(3600)
        except KeyboardInterrupt:
            pass
        return

    app, port = ROLES[args.role]
    log.info("CPU sim %s listening on %s:%s", args.role, args.host, port)
    uvicorn.run(app, host=args.host, port=port, log_level="info")


if __name__ == "__main__":
    main()
