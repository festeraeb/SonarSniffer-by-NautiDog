#!/usr/bin/env python3
"""
CESAROPS Vision Validator — runs on cesarops2 (Quadro P1000 4GB)
Moondream2 for independent visual confirmation.

Endpoints:
  POST /validate  — send a tile patch + scout report, get independent confirmation
  GET  /health    — liveness check

This is the CROSS-VALIDATOR: it must independently confirm what the scout found
WITHOUT being told what to look for (prevents confirmation bias).
"""
import os
import io
import json
import time
import logging
import base64

import torch
from PIL import Image
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import Optional, List
import uvicorn

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("ValidatorP1000")

VISION_ROOT = os.getenv("VISION_MODEL_ROOT", "/data/cesarops/vision_models")
MODEL_ID = os.getenv(
    "VALIDATOR_MODEL",
    os.path.join(VISION_ROOT, "moondream2"),
)
DEVICE = "cuda" if torch.cuda.is_available() else "cpu"
PORT = int(os.getenv("VALIDATOR_PORT", "5572"))

# Load Moondream2
logger.info(f"Loading {MODEL_ID} on {DEVICE}...")
from transformers import AutoModelForCausalLM, AutoTokenizer

tokenizer = AutoTokenizer.from_pretrained(MODEL_ID, trust_remote_code=True)
model = AutoModelForCausalLM.from_pretrained(MODEL_ID, trust_remote_code=True, torch_dtype=torch.float16).to(DEVICE)
model.eval()
logger.info(f"Model loaded. VRAM: {torch.cuda.memory_allocated()/1e9:.1f}GB")

# --- API Models ---

class ValidationRequest(BaseModel):
    tile_id: str
    lat: float = 0.0
    lon: float = 0.0
    image_b64: str
    # We do NOT pass the scout's findings — independent analysis only

class ValidationReport(BaseModel):
    tile_id: str
    has_anomaly: bool
    confidence: float
    shape_analysis: str  # rectangular, linear, irregular, circular, none
    description: str
    material_guess: str  # metal, wood, rock, sand, unknown
    processing_ms: float

# --- Validation Logic ---

VALIDATION_PROMPT = """Look at this satellite image of a water surface. 
Describe any unusual features you see:
- Are there any dark linear shapes that could be submerged objects?
- Are there any bright spots or reflections that seem unnatural?
- Are there any color changes or patterns that differ from surrounding water?
- What shape are any anomalies (rectangular, linear, circular, irregular)?
- What material might cause this (metal, wood, rock, sand)?
Be specific and concise. If nothing unusual, say "No anomalies detected."
"""

def validate_patch(image: Image.Image) -> dict:
    """Run Moondream2 on a tile patch for independent validation."""
    # Moondream2 uses its own image encoding
    enc_image = model.encode_image(image)
    answer = model.answer_question(enc_image, VALIDATION_PROMPT, tokenizer)
    
    # Parse the response
    answer_lower = answer.lower()
    
    # Determine if anomaly detected
    has_anomaly = not ("no anomal" in answer_lower or "nothing unusual" in answer_lower or "no unusual" in answer_lower)
    
    # Shape analysis
    if "rectangular" in answer_lower or "rect" in answer_lower:
        shape = "rectangular"
    elif "linear" in answer_lower or "elongated" in answer_lower or "long" in answer_lower:
        shape = "linear"
    elif "circular" in answer_lower or "round" in answer_lower:
        shape = "circular"
    elif "irregular" in answer_lower:
        shape = "irregular"
    else:
        shape = "none"
    
    # Material guess
    if "metal" in answer_lower or "steel" in answer_lower or "iron" in answer_lower:
        material = "metal"
    elif "wood" in answer_lower:
        material = "wood"
    elif "rock" in answer_lower or "stone" in answer_lower:
        material = "rock"
    elif "sand" in answer_lower:
        material = "sand"
    else:
        material = "unknown"
    
    # Confidence based on specificity of response
    confidence = 0.3  # base
    if has_anomaly:
        confidence = 0.5
        if shape != "none":
            confidence += 0.15
        if material != "unknown":
            confidence += 0.15
        if "dark" in answer_lower and "linear" in answer_lower:
            confidence += 0.1  # strong wreck indicator
    
    return {
        "has_anomaly": has_anomaly,
        "confidence": min(confidence, 0.95),
        "shape_analysis": shape,
        "description": answer[:200],
        "material_guess": material,
    }

# --- FastAPI App ---

app = FastAPI(title="CESAROPS Vision Validator (P1000)", version="1.0.0")

@app.get("/health")
async def health():
    return {
        "service": "validator-p1000",
        "model": MODEL_ID,
        "device": DEVICE,
        "vram_gb": torch.cuda.memory_allocated() / 1e9 if torch.cuda.is_available() else 0,
        "status": "ok"
    }

@app.post("/validate", response_model=ValidationReport)
async def validate(req: ValidationRequest):
    start = time.time()
    
    try:
        img_bytes = base64.b64decode(req.image_b64)
        image = Image.open(io.BytesIO(img_bytes)).convert("RGB")
    except Exception as e:
        raise HTTPException(status_code=400, detail=f"Invalid image: {e}")
    
    result = validate_patch(image)
    elapsed_ms = (time.time() - start) * 1000
    
    return ValidationReport(
        tile_id=req.tile_id,
        has_anomaly=result["has_anomaly"],
        confidence=result["confidence"],
        shape_analysis=result["shape_analysis"],
        description=result["description"],
        material_guess=result["material_guess"],
        processing_ms=elapsed_ms,
    )

if __name__ == "__main__":
    logger.info(f"Starting Validator on port {PORT}")
    uvicorn.run(app, host="0.0.0.0", port=PORT)
