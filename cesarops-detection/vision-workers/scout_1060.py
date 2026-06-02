#!/usr/bin/env python3
"""
CESAROPS Vision Scout — runs on cesarops3 (GTX 1060 6GB)
Florence-2-large for tile anomaly detection.

Endpoints:
  POST /analyze  — send a tile patch, get anomaly classification
  GET  /health   — liveness check

Detects: glint, thermal cold spots, SWIR/NIR sheen, linear features
"""
import os
import io
import sys
import json
import time
import logging
import base64
from pathlib import Path

import torch
import numpy as np
from PIL import Image
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import Optional, List
import uvicorn

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("Scout1060")

# Config — prefer local HF cache under /data/cesarops/vision_models
VISION_ROOT = os.getenv("VISION_MODEL_ROOT", "/data/cesarops/vision_models")
# Florence-2-base (~1.5GB) fits P106 6GB; override with SCOUT_MODEL= path or HF id
MODEL_ID = os.getenv("SCOUT_MODEL", "microsoft/Florence-2-base")
_local = Path(VISION_ROOT) / "Florence-2-base"
if os.getenv("SCOUT_MODEL") is None and _local.is_dir():
    MODEL_ID = str(_local)
# nvidia-smi: GPU0=P106-100 (6GB), GPU1=2060, GPU2=1070 — pin scout to P106
_cuda_dev = os.getenv("CUDA_DEVICE", os.getenv("SCOUT_CUDA_DEVICE", "0"))
if torch.cuda.is_available():
    torch.cuda.set_device(int(_cuda_dev))
    DEVICE = f"cuda:{_cuda_dev}"
else:
    DEVICE = "cpu"
PORT = int(os.getenv("SCOUT_PORT", "5570"))

# Load model
logger.info(f"Loading {MODEL_ID} on {DEVICE}...")
from transformers import AutoProcessor, AutoModelForCausalLM

processor = AutoProcessor.from_pretrained(MODEL_ID, trust_remote_code=True)
model = AutoModelForCausalLM.from_pretrained(MODEL_ID, trust_remote_code=True).to(DEVICE)
model.eval()
logger.info(f"Model loaded. VRAM: {torch.cuda.memory_allocated()/1e9:.1f}GB")

# --- API Models ---

class TilePatch(BaseModel):
    """A tile patch to analyze."""
    tile_id: str
    patch_x: int = 0
    patch_y: int = 0
    lat: float = 0.0
    lon: float = 0.0
    # Base64-encoded image (RGB, any size — will be resized)
    image_b64: str
    # Optional: specific detection task
    task: str = "anomaly_detection"

class ScoutReport(BaseModel):
    """Scout analysis result."""
    tile_id: str
    has_anomaly: bool
    confidence: float
    anomaly_type: str  # glint, thermal_cold, swir_sheen, linear_feature, none
    description: str
    bbox: Optional[List[float]] = None  # [x1, y1, x2, y2] normalized
    processing_ms: float

# --- Detection Logic ---

# Florence-2 requires the task token alone (e.g. "<OD>"), not free-form text.
DETECTION_PROMPTS = {
    "anomaly_detection": "<OD>",
    "glint": "<OD>",
    "thermal": "<OD>",
    "sheen": "<OD>",
    "linear": "<OD>",
    "caption": "<CAPTION>",
    "detailed_caption": "<DETAILED_CAPTION>",
}

MAX_IMAGE_SIDE = int(os.getenv("SCOUT_MAX_IMAGE_SIDE", "1024"))


def _resize_for_model(image: Image.Image) -> Image.Image:
    w, h = image.size
    # Florence-2 OD needs at least ~32px on each side; pad extreme aspect ratios.
    min_side = 32
    if min(w, h) < min_side:
        scale_up = min_side / min(w, h)
        w, h = max(min_side, int(w * scale_up)), max(min_side, int(h * scale_up))
        image = image.resize((w, h), Image.Resampling.LANCZOS)
    m = max(w, h)
    if m <= MAX_IMAGE_SIDE:
        return image
    scale = MAX_IMAGE_SIDE / m
    return image.resize((max(1, int(w * scale)), max(1, int(h * scale)), Image.Resampling.LANCZOS)


def analyze_patch(image: Image.Image, task: str = "anomaly_detection") -> dict:
    """Run Florence-2 on a tile patch."""
    prompt = DETECTION_PROMPTS.get(task, "<OD>")
    image = _resize_for_model(image.convert("RGB"))
    florence_task = "<OD>" if prompt == "<OD>" else prompt

    inputs = processor(text=prompt, images=image, return_tensors="pt").to(DEVICE)
    
    with torch.no_grad():
        generated_ids = model.generate(
            input_ids=inputs["input_ids"],
            pixel_values=inputs["pixel_values"],
            max_new_tokens=256,
            num_beams=3,
        )
    
    generated_text = processor.batch_decode(generated_ids, skip_special_tokens=False)[0]
    parsed = processor.post_process_generation(
        generated_text, task=florence_task, image_size=(image.width, image.height)
    )
    
    # Extract detections
    detections = parsed.get("<OD>", {})
    bboxes = detections.get("bboxes", [])
    labels = detections.get("labels", [])
    
    if bboxes and labels:
        # Found something
        best_bbox = bboxes[0]
        best_label = labels[0]
        confidence = 0.75  # Florence-2 doesn't give confidence scores directly
        
        # Classify anomaly type from label
        label_lower = best_label.lower()
        if "bright" in label_lower or "glint" in label_lower or "reflection" in label_lower:
            anomaly_type = "glint"
        elif "dark" in label_lower or "cold" in label_lower:
            anomaly_type = "thermal_cold"
        elif "sheen" in label_lower or "oil" in label_lower or "iridescent" in label_lower:
            anomaly_type = "swir_sheen"
        elif "linear" in label_lower or "elongated" in label_lower or "hull" in label_lower:
            anomaly_type = "linear_feature"
        else:
            anomaly_type = "unknown"
            confidence = 0.5
        
        return {
            "has_anomaly": True,
            "confidence": confidence,
            "anomaly_type": anomaly_type,
            "description": best_label,
            "bbox": [float(x) for x in best_bbox],
        }
    
    return {
        "has_anomaly": False,
        "confidence": 0.1,
        "anomaly_type": "none",
        "description": "No anomalies detected",
        "bbox": None,
    }

# --- FastAPI App ---

app = FastAPI(title="CESAROPS Vision Scout (1060)", version="1.0.0")

@app.get("/health")
async def health():
    return {
        "service": "scout-1060",
        "model": MODEL_ID,
        "device": DEVICE,
        "vram_gb": torch.cuda.memory_allocated() / 1e9 if torch.cuda.is_available() else 0,
        "status": "ok"
    }

@app.post("/analyze", response_model=ScoutReport)
async def analyze(patch: TilePatch):
    start = time.time()
    
    try:
        # Decode image
        img_bytes = base64.b64decode(patch.image_b64)
        image = Image.open(io.BytesIO(img_bytes)).convert("RGB")
    except Exception as e:
        raise HTTPException(status_code=400, detail=f"Invalid image: {e}")
    
    # Run detection
    result = analyze_patch(image, patch.task)
    elapsed_ms = (time.time() - start) * 1000
    
    return ScoutReport(
        tile_id=patch.tile_id,
        has_anomaly=result["has_anomaly"],
        confidence=result["confidence"],
        anomaly_type=result["anomaly_type"],
        description=result["description"],
        bbox=result["bbox"],
        processing_ms=elapsed_ms,
    )

if __name__ == "__main__":
    logger.info(f"Starting Scout on port {PORT}")
    uvicorn.run(app, host="0.0.0.0", port=PORT)
