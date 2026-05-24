#!/usr/bin/env python3
"""
CESAROPS Scout — YOLO11 fast path (optional ultralytics).

Same contract as scout_1060.py: POST /analyze, GET /health.
Falls back to numpy heuristics when ultralytics or weights are missing.

Env:
  YOLO_MODEL   — path to .pt (default: yolo11n.pt or auto-download)
  SCOUT_PORT   — default 5570
  DEVICE       — cuda:0 | cpu
"""
from __future__ import annotations

import base64
import io
import logging
import os
import time
from typing import List, Optional

import numpy as np
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
import uvicorn

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
log = logging.getLogger("ScoutYOLO11")

try:
    from PIL import Image
except ImportError:
    Image = None  # type: ignore

PORT = int(os.getenv("SCOUT_PORT", "5570"))
DEVICE = os.getenv("DEVICE", "cuda" if os.getenv("CUDA_VISIBLE_DEVICES") != "" else "cpu")
YOLO_MODEL = os.getenv("YOLO_MODEL", "yolo11n.pt")

_yolo = None
_yolo_backend = "heuristic"


def _load_yolo():
    global _yolo, _yolo_backend
    if _yolo is not None:
        return _yolo
    try:
        from ultralytics import YOLO

        _yolo = YOLO(YOLO_MODEL)
        _yolo_backend = f"yolo11:{YOLO_MODEL}"
        log.info("Loaded YOLO model %s on %s", YOLO_MODEL, DEVICE)
    except Exception as e:
        log.warning("YOLO unavailable (%s) — using CPU heuristics", e)
        _yolo = False
        _yolo_backend = "heuristic"
    return _yolo


class TilePatch(BaseModel):
    tile_id: str
    patch_x: int = 0
    patch_y: int = 0
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


def _heuristic(gray: np.ndarray) -> dict:
    std = float(np.std(gray))
    edge = float(np.mean(np.abs(np.diff(gray, axis=1))))
    bright = float(np.percentile(gray, 99))
    score = min(0.95, 0.3 + std / 80.0 + edge / 40.0 + (bright - float(np.mean(gray))) / 200.0)
    if score < 0.55:
        return {
            "has_anomaly": False,
            "confidence": score,
            "anomaly_type": "none",
            "description": "No significant features (heuristic)",
            "bbox": None,
        }
    atype = "glint" if bright > float(np.mean(gray)) + 30 else "linear_feature"
    return {
        "has_anomaly": True,
        "confidence": score,
        "anomaly_type": atype,
        "description": f"heuristic {atype}",
        "bbox": [0.25, 0.25, 0.75, 0.75],
    }


def _analyze_yolo(image: Image.Image) -> dict:
    model = _load_yolo()
    if not model:
        gray = np.asarray(image.convert("L"), dtype=np.float32)
        return _heuristic(gray)

    results = model.predict(image, verbose=False, device=0 if "cuda" in DEVICE else "cpu")
    if not results or results[0].boxes is None or len(results[0].boxes) == 0:
        return {
            "has_anomaly": False,
            "confidence": 0.2,
            "anomaly_type": "none",
            "description": "YOLO: no objects",
            "bbox": None,
        }

    boxes = results[0].boxes
    best_i = int(boxes.conf.argmax()) if len(boxes.conf) else 0
    conf = float(boxes.conf[best_i])
    xyxy = boxes.xyxy[best_i].tolist()
    w, h = image.size
    norm = [xyxy[0] / w, xyxy[1] / h, xyxy[2] / w, xyxy[3] / h]
    cls = int(boxes.cls[best_i]) if boxes.cls is not None else -1
    names = getattr(results[0], "names", {}) or {}
    label = names.get(cls, "object")
    atype = "glint" if "bright" in label.lower() else "linear_feature"
    return {
        "has_anomaly": conf >= 0.35,
        "confidence": conf,
        "anomaly_type": atype if conf >= 0.35 else "none",
        "description": f"YOLO {label} conf={conf:.2f}",
        "bbox": norm,
    }


app = FastAPI(title="CESAROPS Scout YOLO11", version="1.0.0")


@app.on_event("startup")
async def startup():
    _load_yolo()


@app.get("/health")
async def health():
    return {
        "service": "scout-yolo11",
        "backend": _yolo_backend,
        "model": YOLO_MODEL,
        "device": DEVICE,
        "status": "ok",
    }


@app.post("/analyze", response_model=ScoutReport)
async def analyze(patch: TilePatch):
    if Image is None:
        raise HTTPException(status_code=500, detail="PIL required")
    t0 = time.time()
    try:
        raw = base64.b64decode(patch.image_b64)
        image = Image.open(io.BytesIO(raw)).convert("RGB")
    except Exception as e:
        raise HTTPException(status_code=400, detail=str(e)) from e

    out = _analyze_yolo(image)
    return ScoutReport(
        tile_id=patch.tile_id,
        processing_ms=(time.time() - t0) * 1000,
        **out,
    )


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=PORT)
