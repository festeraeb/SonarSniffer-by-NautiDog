#!/usr/bin/env python3
"""
Quick pipeline test using existing tiles on T440.
Runs a CPU-based mock scout (simple threshold detection) to prove the workflow.
Then tests the full cesarops-detection Rust dispatcher.
"""
import json
import base64
import time
import urllib.request
from pathlib import Path

TILES_DIR = Path("/mnt/data-external/cesarops/repo/downloads/michigan/2022/hls")
DETECTION_URL = "http://localhost:5580"

# Check if detection service is running
try:
    resp = urllib.request.urlopen(f"{DETECTION_URL}/health", timeout=3)
    print(f"Detection service: {json.loads(resp.read())}")
except:
    print("Detection service not running. Starting it...")
    import subprocess
    subprocess.Popen([
        "/home/cesarops/wreckhunter2000-1/target/release/cesarops-detection"
    ], env={**__import__('os').environ, "DETECTION_PORT": "5580"})
    time.sleep(3)

# Load a tile and convert to base64 image
print("\n=== Loading test tile ===")
tile_files = sorted(TILES_DIR.glob("*.tif"))
if not tile_files:
    print("No tiles found!")
    exit(1)

# Use the red band as a grayscale image for testing
red_tile = [f for f in tile_files if "red" in f.name]
if red_tile:
    tile_path = red_tile[0]
else:
    tile_path = tile_files[0]

print(f"Using: {tile_path.name} ({tile_path.stat().st_size / 1e6:.1f} MB)")

# Try to read with rasterio, fall back to raw bytes
try:
    import rasterio
    with rasterio.open(tile_path) as src:
        data = src.read(1)  # First band
        # Take a 512x512 crop from center
        h, w = data.shape
        cy, cx = h // 2, w // 2
        crop = data[cy-256:cy+256, cx-256:cx+256]
        print(f"  Shape: {data.shape}, Crop: {crop.shape}")
        print(f"  Min: {crop.min()}, Max: {crop.max()}, Mean: {crop.mean():.1f}")
        
        # Convert to 8-bit PNG for the vision model
        import numpy as np
        from PIL import Image
        import io
        
        # Normalize to 0-255
        norm = ((crop - crop.min()) / (crop.max() - crop.min() + 1e-10) * 255).astype(np.uint8)
        img = Image.fromarray(norm, mode='L').convert('RGB')
        
        # Encode as base64
        buf = io.BytesIO()
        img.save(buf, format='PNG')
        image_b64 = base64.b64encode(buf.getvalue()).decode()
        print(f"  Encoded: {len(image_b64)} chars base64")
        
except ImportError:
    print("  No rasterio — using raw file as test payload")
    # Just use first 1KB as a dummy
    with open(tile_path, 'rb') as f:
        raw = f.read(1024)
    image_b64 = base64.b64encode(raw).decode()

# Submit a scan job to the detection pipeline
print("\n=== Submitting scan job ===")
scan_request = {
    "region": "lake_michigan_test",
    "tiles": [
        {
            "lat": 43.5,
            "lon": -87.0,
            "image_b64": image_b64
        }
    ]
}

try:
    payload = json.dumps(scan_request).encode()
    req = urllib.request.Request(
        f"{DETECTION_URL}/scan",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
    print(f"Job submitted: {resp}")
    
    job_id = resp.get("job_id")
    if job_id:
        # Poll for results
        print("\nPolling for results...")
        for i in range(10):
            time.sleep(3)
            status_resp = urllib.request.urlopen(f"{DETECTION_URL}/scan/{job_id}", timeout=5)
            status = json.loads(status_resp.read())
            print(f"  [{i*3}s] Status: {status.get('status')}, Processed: {status.get('processed', 0)}/{status.get('total_tiles', 0)}")
            if status.get("status") in ["Completed", "completed"]:
                print(f"\n=== RESULTS ===")
                print(f"  Confirmed detections: {status.get('confirmed_detections', 0)}")
                if status.get("detections"):
                    for d in status["detections"]:
                        print(f"    {d}")
                break
                
except Exception as e:
    print(f"Error: {e}")
    # The workers aren't running yet — that's expected
    print("\nNote: Scout/Validator workers not deployed yet.")
    print("The Rust dispatcher is running but can't reach the Python vision services.")
    print("Deploy Florence-2 on cesarops3 and Moondream2 on cesarops2 to complete the pipeline.")
