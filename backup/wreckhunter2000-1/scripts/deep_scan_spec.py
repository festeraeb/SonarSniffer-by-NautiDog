#!/usr/bin/env python3
"""
Thought Engine simulation: Deep codebase scan → full sensor spec → roadmap.
Step 1 (THINK): Query nautivecs exhaustively for ALL sensor/detection code
Step 2 (EXECUTE): Feed to 35B with full context for comprehensive spec
"""
import json
import urllib.request
import os

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"
OUTPUT = "/home/cesarops/wreckhunter2000-1/docs/full_sensor_scan_spec.md"

# === STEP 1: THINK — Deep codebase scan ===
print("=== THINKING: Deep nautivecs scan for ALL sensor/detection code ===")
queries = [
    # Magnetics / dipole
    "dipole scan magnetic anomaly ferrous hull curvelet",
    "fdct kernels wgpu shader workgroup P100 spatial",
    "aeromagnetic survey grid detection threshold",
    # ICESat-2 / SWOT / altimetry
    "ICESat ATL03 ATL12 bathymetry laser altimetry",
    "SWOT surface water topography flow disruption",
    # Thermal
    "thermal contrast heat sink Landsat Band 10 ECOSTRESS",
    "temperature anomaly steel hull diurnal cooling",
    # SAR
    "SAR Sentinel-1 texture roughness backscatter",
    "synthetic aperture radar wave pattern disruption",
    # Optical
    "optical Sentinel-2 glint sun shallow water hull outline",
    "cloud mask atmospheric correction spectral band",
    # Weather / temporal
    "weather NOAA buoy wind storm seiche water level",
    "temporal stack tile weight post storm calm",
    # Pipeline / orchestration
    "scan worker queue tile region download process",
    "pipeline dispatch anomaly report confidence score",
    # Data sources
    "Earthdata Copernicus USGS download API credentials",
    "satellite data sources JSON configuration",
]

all_context = []
for q in queries:
    payload = json.dumps({"query": q, "top_k": 3, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        if resp.get("context_block"):
            all_context.append(f"### Query: {q}\n{resp['context_block']}")
        results_count = len(resp.get('results', []))
        print(f"  [{q[:55]}] -> {results_count} results")
    except Exception as e:
        print(f"  [{q[:55]}] -> ERROR: {e}")

combined_context = "\n\n".join(all_context)
print(f"\nTotal THINK context: {len(combined_context)} chars from {len(all_context)} successful queries")

# === STEP 2: EXECUTE — Feed to 35B ===
print("\n=== EXECUTING: Sending to 35B for full sensor spec + roadmap ===")

system_msg = f"""You are CESAROPS (Qwen3.6-35B-A3B) in SCAN MODE. You have just completed a deep scan of your own codebase via nautivecs. Below is EVERYTHING found related to sensors, detection methods, data sources, and pipeline code.

Your job: Use this real code context to produce the DEFINITIVE search parameter specification for a blind validation scan. Do NOT hallucinate capabilities you don't have. Reference the actual code below.

=== DEEP CODEBASE SCAN RESULTS ===
{combined_context}
=== END CODEBASE SCAN ==="""

user_msg = """Based on your deep codebase scan, produce THREE documents:

## DOCUMENT 1: Complete Sensor Stack Specification

For EACH sensor type you found in the codebase, specify:
- What code exists for it (cite actual files/functions)
- What data source it uses (API endpoint, credentials needed)
- What physical phenomenon it detects
- Optimal acquisition conditions (weather, time of day, season)
- How it integrates with the temporal stacking pipeline
- Current implementation status (working / partial / stub / missing)

Cover ALL of these:
1. Optical (Sentinel-2, Landsat)
2. SAR (Sentinel-1)
3. Thermal IR (Landsat Band 10, ECOSTRESS)
4. Laser Altimetry (ICESat-2 ATL03/ATL12)
5. Surface Topography (SWOT)
6. Aeromagnetic/Dipole (your WGSL shaders)
7. Weather/Environmental (NOAA buoys, water level gauges)

## DOCUMENT 2: Optimal Search Parameters for Blind Validation

Given the ACTUAL capabilities in your codebase (not theoretical), specify:
- Which sensors to use for Mackinac Straits vs Lake Erie (they have different characteristics)
- Exact date ranges to pull (based on known storm events in 2024-2025)
- Confidence thresholds for each sensor type
- How to fuse multi-sensor detections (weighted voting? intersection? union?)
- Minimum temporal stack depth per sensor
- Expected false positive rate per sensor
- How to prioritize: which sensor runs first, which confirms?

## DOCUMENT 3: Roadmap + New Tools Needed

Based on what you found in the codebase vs what the scan strategy REQUIRES:
- What's MISSING? What code needs to be written?
- What external tools/libraries would help? (new Rust crates, Python packages, APIs)
- What data sources are referenced but not implemented?
- Priority order: what gives the biggest detection improvement for least effort?
- Any novel approaches you can think of that aren't in the current design?
- Hardware utilization: are we using the P100s optimally? Could the 1070/1060 help?

Be BRUTALLY HONEST about what works and what's vaporware. This roadmap determines what we build next."""

request_body = {
    "model": "koboldcpp/Qwen3.6-35B-A3B-MXFP4_MOE",
    "messages": [
        {"role": "system", "content": system_msg},
        {"role": "user", "content": user_msg}
    ],
    "max_tokens": 16384,
    "temperature": 0.5,
    "top_p": 0.95,
    "presence_penalty": 1.2
}

print(f"System: {len(system_msg)} chars | User: {len(user_msg)} chars")
print("Generating full sensor spec + roadmap (15-20 min)...")

payload = json.dumps(request_body).encode()
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")

try:
    resp = json.loads(urllib.request.urlopen(req, timeout=2400).read())
    content = resp["choices"][0]["message"]["content"]

    with open(OUTPUT, "w") as f:
        f.write(content)

    print(f"\n=== GENERATED ===")
    print(f"Length: {len(content)} chars (~{len(content.split())} words)")
    print(f"Saved to: {OUTPUT}")
    print(f"\nFirst 500 chars:")
    print(content[:500])

except Exception as e:
    print(f"ERROR: {e}")
    import traceback
    traceback.print_exc()
