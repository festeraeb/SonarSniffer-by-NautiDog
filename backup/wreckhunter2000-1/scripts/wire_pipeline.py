#!/usr/bin/env python3
"""
After nautivecs finishes indexing, ask the 35B to wire all existing pieces together.
Runs on T440 — waits for indexing to complete, then dispatches the integration task.
"""
import json
import time
import urllib.request
import subprocess

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"
OUTPUT = "/mnt/data-external/cesarops/analysis-v2/pipeline_wiring.md"

def nautivecs_ready():
    """Check if nautivecs has finished indexing (chunks > 4000 means full codebase)."""
    try:
        resp = json.loads(urllib.request.urlopen(f"{NAUTIVECS}/health", timeout=5).read())
        return resp.get("chunks", 0)
    except:
        return 0

def query_nautivecs(query, top_k=5):
    payload = json.dumps({"query": query, "top_k": top_k, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        return resp.get("context_block", "")
    except:
        return ""

def call_35b(system, user, max_tokens=16384):
    payload = json.dumps({
        "model": "auto",
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "max_tokens": max_tokens,
        "temperature": 0.4,
        "stream": False
    }).encode()
    req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    resp = json.loads(urllib.request.urlopen(req, timeout=1200).read())
    return resp["choices"][0]["message"]["content"]

# Wait for indexing to complete
print("Waiting for nautivecs indexing to complete...")
while True:
    chunks = nautivecs_ready()
    if chunks > 4000:
        print(f"Indexing complete: {chunks} chunks")
        break
    print(f"  {chunks} chunks indexed, waiting...")
    time.sleep(30)

# Restart nautivecs server to pick up new index
subprocess.run(["bash", "-c", "echo cesarops | sudo -S systemctl restart nautivecs-server"], capture_output=True)
time.sleep(5)

# Now query for all the key pieces
print("\n=== Gathering context for pipeline wiring ===")
queries = [
    # Existing implementations to wire together
    "drift engine numba analyzer ml_predictor sarops",
    "slicer tiles vrt_slicer anchor geotiff gdal_warp",
    "nauticuvs forward inverse coeffs curvelet transform",
    "triple_lock triple_lock_fusion tpu_client tpu_server",
    "deep_water_detection wreck_vs_obstruction_classifier ghost_scan",
    "background_probe orchestrator scan worker queue",
    "weather_fetcher glos_analyzer buoy_analog",
    "sentinel_hunt detect config glos thermal",
    # Pipeline coordination
    "sovereign cloud pipeline dispatch pass scout analyst",
    "mission control scan mode weather filter download",
]

all_context = []
for q in queries:
    ctx = query_nautivecs(q, top_k=4)
    if ctx:
        all_context.append(f"### {q}\n{ctx}")
    print(f"  [{q[:50]}] -> {'found' if ctx else 'empty'}")

combined = "\n\n".join(all_context)
print(f"\nTotal context: {len(combined)} chars")

# Ask the 35B to wire it all together
print("\n=== Asking 35B to wire the pipeline ===")

system_msg = f"""You are CESAROPS (Qwen3.6-35B). You have access to the FULL codebase via nautivecs.
The code already exists — drift correction, slicer, curvelets, triple-lock, TPU, ML detection.
Your job is to WIRE THEM TOGETHER into a working pipeline.

Here is what nautivecs found in the codebase:
{combined[:20000]}"""

user_msg = """The existing code is scattered across repos but ALL the pieces exist:

EXISTING (Python):
- cesarops/src/cesarops/drift/ — full drift engine (engine.py, engine_numba.py, analyzer.py, ml_predictor.py)
- cesarops/src/cesarops/triple_lock.py — triple lock detection
- cesarops/src/cesarops/orchestrator.py — pipeline orchestration
- wreckhunter2000/cesarops-core/tpu_client.py + tpu_server.py — TPU communication
- ml/inference/ — deep water detection, wreck classifier, ghost scan
- pipelines/satellite/ — tile extraction, synthetic tiles, drift tracking
- weather_service.py — NOAA buoy integration

EXISTING (Rust):
- cesarops-slicer/src/tiles/ — slicer.rs, vrt_slicer.rs, anchor.rs (sub-pixel alignment)
- nauticuvs/src/ — forward.rs, inverse.rs, coeffs.rs (curvelet transforms)
- sentinel_hunt_src/src/ — detect.rs, glos.rs, config.rs

NEW (needs writing):
- Vision AI service (Florence-2 on 1060, Moondream2 on P1000)
- P100 batch orchestration (how many tiles fit in VRAM, when to load/unload LLM)
- Agent steering docs (teach the AI when to call each tool)

TASK: Write a WIRING DOCUMENT that:

1. Maps each pipeline stage to its EXISTING implementation file
2. Shows the data flow: weather check → download → drift correct → slice → detect → triple-lock → report
3. Identifies the 3-4 integration points that need NEW glue code
4. Writes the glue code (Python or Rust) that connects the existing pieces
5. Produces a steering document for nautivecs that teaches the agents the tool inventory
6. Calculates P100 VRAM budget: how many tile bands × how many days fit in 32GB

Be specific. Reference actual file paths. Write actual integration code.
The goal: after this document, we can run the blind validation scan by executing a single command."""

print("Generating (this is the big one — 10-15 min)...")
result = call_35b(system_msg, user_msg)

with open(OUTPUT, "w") as f:
    f.write(f"# Pipeline Wiring Document\n\n{result}\n")

print(f"\nDone. {len(result)} chars written to {OUTPUT}")
