#!/usr/bin/env python3
"""
Ask the 35B to design and write a UNIFIED adaptive pipeline crate.
Not one-run-fits-all. Not separate code per detection type.
One crate with knobs that an LLM worker bee tunes per mission.

The AI worker is an expert at:
- Choosing band combinations (SWIR for hydrocarbons, thermal for heat sinks, blue for bathymetry)
- Setting thresholds (glint sensitivity, plume contrast, cold spot delta)
- Deciding what to look for (clear spots = zebra mussels, plumes = post-storm, ripples = submerged structure)
- Adapting between missions without code changes

The base code handles the mechanics. The AI handles the strategy.
"""
import json
import urllib.request
import time

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"
OUTPUT = "/mnt/data-external/cesarops/analysis-v2/adaptive_pipeline.md"

def query_nautivecs(query, top_k=4):
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

# Gather context
print("=== Gathering context ===")
contexts = []
for q in [
    "slicer tiles anchor vrt geotiff bands config",
    "detect config threshold glint hydrocarbon thermal",
    "triple lock consensus verify confidence",
    "orchestrator mission spec knobs weather filter",
    "synthetic tile anomaly delta stack weight bands",
    "scan strategy calm post storm thermal contrast SAR",
]:
    ctx = query_nautivecs(q)
    if ctx:
        contexts.append(ctx)
    print(f"  [{q[:50]}] -> {'found' if ctx else 'empty'}")

combined = "\n\n".join(contexts)
print(f"Context: {len(combined)} chars")

# The big ask
print("\n=== Sending to 35B ===")

system_msg = f"""You are CESAROPS (Qwen3.6-35B). You are designing a UNIFIED ADAPTIVE detection pipeline.

KEY PHILOSOPHY: One codebase, many missions. The code doesn't change between runs.
What changes is the CONFIGURATION — and an LLM worker bee (DeepSeek-R1 on the 1070)
is the expert knob-tuner that decides the config for each mission based on:
- Current weather conditions
- What we're looking for (hydrocarbons, heat sinks, clear spots, plumes, ripples)
- Which bands to combine
- What thresholds to use

The base Rust crate handles:
- Loading any combination of spectral bands from GeoTIFF
- Applying configurable detection algorithms (threshold, ratio, delta, texture)
- Running the slicer with configurable tile sizes
- Feeding results to the triple-lock

The AI worker handles:
- Reading weather data and deciding what detection mode to use
- Choosing band combinations (e.g., SWIR/NIR ratio for hydrocarbons, B10 delta for thermal)
- Setting confidence thresholds based on conditions
- Interpreting results and deciding if re-scan is needed

Existing codebase context (from nautivecs, 12136 chunks):
{combined[:15000]}

RUST CODEGEN RULES:
- Enum dispatch not trait objects for async
- Compute values before moving into structs
- Vec<(&str, &str)> for HTTP params
- Annotate .collect() types"""

user_msg = """Design and write a UNIFIED ADAPTIVE detection crate: `cesarops-adaptive`

This is NOT multiple detection modes with separate code paths.
This is ONE configurable pipeline where the AI worker tunes the knobs.

## The Knob System

```rust
pub struct MissionConfig {
    // What are we looking for?
    pub detection_mode: DetectionMode,
    // Which bands to load and how to combine them
    pub band_recipe: BandRecipe,
    // Sensitivity thresholds
    pub thresholds: Thresholds,
    // Temporal stacking parameters
    pub stacking: StackConfig,
}

pub enum DetectionMode {
    HydrocarbonSheen,   // SWIR dark absorption (oil/fuel leaks)
    ThermalSink,        // Cold spot in thermal (steel hull at depth)
    ClearWater,         // Unusually clear patch (zebra mussels on wreck)
    SedimentPlume,      // Post-storm turbidity wake (hull disrupts flow)
    SurfaceRipple,      // Persistent ripple on calm days (submerged structure)
    Glint,              // Specular reflection (metal at shallow depth)
    Custom(String),     // AI-defined custom detection
}

pub struct BandRecipe {
    pub primary: Band,      // Main detection band
    pub secondary: Band,    // Ratio/difference partner
    pub operation: BandOp,  // How to combine (ratio, difference, index)
    pub normalize: bool,
}

pub enum BandOp {
    Ratio,          // primary / secondary (e.g., NDWI = green/nir)
    Difference,     // primary - secondary (e.g., thermal delta)
    Index,          // (p - s) / (p + s) (normalized difference)
    Single,         // Just use primary band
    FalseColor,     // RGB composite for vision model
}
```

## What to write:

=== FILE: cesarops-adaptive/Cargo.toml ===
[standard deps: reqwest, tokio, serde, axum, ndarray, geotiff/tiff reading]

=== FILE: cesarops-adaptive/src/main.rs ===
[axum server that accepts mission configs and runs the pipeline]

=== FILE: cesarops-adaptive/src/config.rs ===
[MissionConfig, DetectionMode, BandRecipe, Thresholds, StackConfig — all serializable]

=== FILE: cesarops-adaptive/src/bands.rs ===
[Band loading from GeoTIFF, band math operations (ratio, difference, index)]

=== FILE: cesarops-adaptive/src/detect.rs ===
[Unified detection: takes band-math output + thresholds, produces anomaly map]

=== FILE: cesarops-adaptive/src/worker.rs ===
[The AI worker client — calls DeepSeek-R1 on 1070 to get MissionConfig for current conditions]

=== FILE: cesarops-adaptive/src/presets.rs ===
[Pre-built configs for common missions: hydrocarbon_scan, thermal_survey, post_storm_plume, calm_day_baseline]

The AI worker (DeepSeek-R1) gets called like:
"Current conditions: wind 3mph, 2 days post-storm, water temp 18C, target area: Kelley's Island.
What detection mode, band recipe, and thresholds should I use?"

And it returns a MissionConfig JSON that the pipeline executes.

Write COMPLETE, COMPILABLE Rust. This is the capstone crate."""

print("Generating adaptive pipeline (10-15 min)...")
start = time.time()
result = call_35b(system_msg, user_msg)
elapsed = time.time() - start
print(f"Generated: {len(result)} chars in {elapsed:.0f}s")

with open(OUTPUT, "w") as f:
    f.write(f"# Adaptive Pipeline — Unified Detection Crate\n\n{result}\n")

# Parse and write files
import re
file_pattern = r'=== FILE: (.+?) ==='
parts = re.split(file_pattern, result)
files_written = 0
if len(parts) > 1:
    for i in range(1, len(parts), 2):
        filename = parts[i].strip()
        file_content = parts[i+1].strip() if i+1 < len(parts) else ""
        file_content = re.sub(r'^```\w*\n?', '', file_content)
        file_content = re.sub(r'\n?```\s*$', '', file_content)
        file_content = file_content.strip()
        
        from pathlib import Path
        filepath = Path(f"/home/cesarops/wreckhunter2000-1/{filename}")
        filepath.parent.mkdir(parents=True, exist_ok=True)
        filepath.write_text(file_content + "\n")
        print(f"  Written: {filename} ({len(file_content)} bytes)")
        files_written += 1

print(f"\nFiles written: {files_written}")
print(f"Output: {OUTPUT}")
