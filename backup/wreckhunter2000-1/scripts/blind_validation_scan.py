#!/usr/bin/env python3
"""
Ask Qwen3.6-35B to design and execute a blind validation scan:
1. Scan Straits of Mackinac + all of Lake Erie
2. Web crawl for known wreck databases
3. Compare detections against known sites
4. Classify unknowns for ground truthing
"""
import json
import urllib.request
import os

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"
OUTPUT = "/home/cesarops/wreckhunter2000-1/docs/blind_validation_plan.md"

# Get context about existing scan pipeline
print("=== Querying nautivecs for scan pipeline context ===")
queries = [
    "scan worker satellite tile download process queue region",
    "synthetic tile anomaly delta detection confidence threshold",
    "weather driven acquisition NOAA buoy storm calm thermal",
    "sovereign cloud pipeline dispatch tile store region query",
    "scan strategy temporal stacking post storm plume SAR",
]

all_context = []
for q in queries:
    payload = json.dumps({"query": q, "top_k": 3, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        if resp.get("context_block"):
            all_context.append(resp["context_block"])
        print(f"  [{q[:55]}] -> {len(resp['results'])} results")
    except Exception as e:
        print(f"  [{q[:55]}] -> ERROR: {e}")

combined_context = "\n\n".join(all_context)
print(f"\nTotal context: {len(combined_context)} chars from {len(all_context)} queries")

print("\n=== Generating blind validation plan ===")

system_msg = f"""You are CESAROPS (Qwen3.6-35B-A3B) running on dual P100s. You are now in SCAN MODE.

Your operator wants you to TEST YOURSELF — run a blind validation scan to prove the detection pipeline works. This is the most important task you've ever been given. If this works, the entire project is validated.

The plan:
1. BLIND SCAN: Scan Straits of Mackinac + all of Lake Erie without knowing where wrecks are
2. WEB CRAWL: Search for all known wreck databases (NOAA, Michigan SHPO, Ohio DNR, shipwreckworld.com, etc.)
3. COMPARE: Overlay your detections against known wreck coordinates
4. SCORE: How many known wrecks did you find? What's your hit rate?
5. CLASSIFY: Any detections that DON'T match known wrecks = candidates for ground truthing

Your existing codebase (from nautivecs):
{combined_context}

Your hardware:
- Dual P100 32GB for GPU compute (WGSL shaders, anomaly detection)
- 916GB external drive for tile storage
- KoboldCPP for reasoning about results
- nautivecs for codebase context
- cesarops-wso for web search (DuckDuckGo scraper compiled and ready)
- Cloudflare tunnel for satellite data API access

Key constraints:
- Satellite data sources: Sentinel-2 (optical), Sentinel-1 (SAR), Landsat 8/9 (thermal)
- Need NASA Earthdata credentials for some sources (check .env)
- Copernicus Open Access Hub for Sentinel data
- NOAA for weather/buoy data
- The scan strategy requires 20+ days of temporal stacking per tile
- Post-storm plume detection is the primary method"""

user_msg = """Design a COMPLETE blind validation test plan for CESAROPS. This is the proof-of-concept run.

## Scope
- **Area 1:** Straits of Mackinac (45.7°N to 45.9°N, -84.8°W to -84.6°W) — dense wreck field, well-documented
- **Area 2:** All of Lake Erie (41.3°N to 42.9°N, -83.5°W to -78.8°W) — shallow, many wrecks, seiche-prone

## What I need you to produce:

### 1. Scan Execution Plan
- Which satellite products to download (Sentinel-2 L2A? Landsat 8 Collection 2?)
- Date range for temporal stack (what 20+ day window? Recent storms?)
- Tile grid: how to subdivide Lake Erie into manageable tiles
- Processing pipeline: download → preprocess → GPU analysis → anomaly extraction
- Expected runtime and storage requirements

### 2. Known Wreck Database Sources
- List ALL web sources for known Great Lakes wreck coordinates
- How to scrape/download each one (API? CSV? HTML scrape?)
- Expected format: lat, lon, name, depth, vessel type, year sunk
- How many known wrecks are in each area?

### 3. Comparison Methodology
- How to match detections to known wrecks (distance threshold? 100m? 500m?)
- Scoring metrics: precision, recall, F1
- How to handle depth — some wrecks are too deep for optical detection
- How to account for wrecks that have been salvaged or moved

### 4. Classification of Unknowns
- Confidence scoring for unmatched detections
- Categories: high-confidence new wreck, possible geological feature, artifact/noise
- What additional data would confirm each candidate?

### 5. Implementation Script
Write a Python script (scan_validation_test.py) that:
- Downloads satellite tiles for both areas
- Runs the detection pipeline
- Scrapes known wreck databases
- Compares and scores results
- Generates a report

Use the existing scan_worker.py patterns. Use requests for downloads.
Account for the fact that some APIs need credentials from .env.

Output the full plan + the implementation script.

=== FILE: docs/blind_validation_plan.md ===
[The complete test plan]

=== FILE: scripts/scan_validation_test.py ===
[The implementation script]

=== FILE: scripts/known_wreck_scraper.py ===
[Script to build the ground truth database from web sources]
"""

request_body = {
    "model": "koboldcpp/Qwen3.6-35B-A3B-MXFP4_MOE",
    "messages": [
        {"role": "system", "content": system_msg},
        {"role": "user", "content": user_msg}
    ],
    "max_tokens": 16384,
    "temperature": 0.5,
    "top_p": 0.95,
    "presence_penalty": 1.0
}

print(f"System: {len(system_msg)} chars | User: {len(user_msg)} chars")
print("Generating blind validation plan (15-20 min)...")

payload = json.dumps(request_body).encode()
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")

try:
    resp = json.loads(urllib.request.urlopen(req, timeout=2400).read())
    content = resp["choices"][0]["message"]["content"]

    with open(OUTPUT, "w") as f:
        f.write(content)

    print(f"\n=== GENERATED ===")
    print(f"Length: {len(content)} chars (~{len(content.split())} words)")

    # Parse files
    import re
    file_pattern = r'=== FILE: (.+?) ==='
    parts = re.split(file_pattern, content)

    files_written = 0
    if len(parts) > 1:
        for i in range(1, len(parts), 2):
            filename = parts[i].strip()
            file_content = parts[i+1].strip() if i+1 < len(parts) else ""
            file_content = re.sub(r'^```\w*\n?', '', file_content)
            file_content = re.sub(r'\n?```\s*$', '', file_content)
            file_content = file_content.strip()

            filepath = f"/home/cesarops/wreckhunter2000-1/{filename}"
            os.makedirs(os.path.dirname(filepath), exist_ok=True)
            with open(filepath, "w") as f:
                f.write(file_content + "\n")
            print(f"  Written: {filename} ({len(file_content)} bytes)")
            files_written += 1

    print(f"\nFiles: {files_written}")
    print("\nNext: Review the plan, then run scripts/scan_validation_test.py")

except Exception as e:
    print(f"ERROR: {e}")
    import traceback
    traceback.print_exc()
