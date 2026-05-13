#!/usr/bin/env python3
"""
CESAROPS Deep Analysis v2 — File-by-file codebase scan + Triple-Lock implementation.

This version:
1. Scans EVERY .rs, .py, .wgsl file in the local repo
2. Pulls from GitHub repos (festeraeb) — nauticuvs, CESARops, wreckhunter2000
3. Feeds the Triple-Lock detection architecture to the 35B
4. Has the 35B write the actual Rust validation coordinator
5. Includes TPU jitter model spec for the VM

Runs on T440 — uses both DeepSeek-R1 (fast COT on cesarops2) and Qwen3.6-35B (heavy impl on P100s).
"""
import json
import time
import os
import urllib.request
from pathlib import Path

DEEPSEEK = "http://100.102.158.111:5555/v1"  # DeepSeek-R1 on 1070
QWEN35B = "http://localhost:5001/v1"  # Qwen3.6-35B on P100s
NAUTIVECS = "http://localhost:5003"  # nautivecs on T440
REPO_DIR = Path("/mnt/data-external/cesarops/repo-full")
OUTPUT_DIR = Path("/mnt/data-external/cesarops/analysis-v2")
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

GITHUB_REPOS = [
    "festeraeb/nauticuvs",
    "festeraeb/CESARops",
    "festeraeb/wreckhunter2000",
]

# Triple-Lock architecture to feed to the models
TRIPLE_LOCK = """
## Triple-Lock Detection Pipeline Architecture

Three independent systems must agree before a detection is confirmed:

1. SCOUT (GTX 1060, Florence-2): Glint, thermal cold spots, SWIR/NIR sheen, linear features
2. CROSS-VALIDATOR (P1000, Moondream2): Independent visual confirmation of anomaly shape/structure
3. JITTER ANALYST (Coral TPU in VM): Thermal time-series oscillation analysis — identifies material type (steel/iron vs rock/sand)

Only when all three agree does the REASONER (DeepSeek-R1 on 1070) produce a confirmed detection.

The TPU jitter check is the "kill shot" — a sandbar fools vision models but has no thermal oscillation.
Steel/iron cargo at depth produces measurable thermal jitter (proven: identified rail iron at 500ft).

Write Rust code for:
- DetectionPipeline struct with async process_tile()
- Node trait for each hardware endpoint
- JitterRequest/JitterResponse for TPU VM communication
- MissionAction enum (Standby, Investigate, Confirmed, Alert)
- Integration with sovereign-cloud dispatch
"""

def call_llm(endpoint, system, user, max_tokens=8192, temperature=0.4):
    """Call an LLM endpoint."""
    payload = json.dumps({
        "model": "auto",
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "max_tokens": max_tokens,
        "temperature": temperature,
        "stream": False
    }).encode()
    req = urllib.request.Request(
        f"{endpoint}/chat/completions",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=600).read())
        return resp["choices"][0]["message"]["content"]
    except Exception as e:
        return f"ERROR: {e}"

def query_nautivecs(query, top_k=5):
    """Search codebase via nautivecs."""
    payload = json.dumps({"query": query, "top_k": top_k, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        return resp.get("context_block", "")
    except:
        return ""

def list_source_files(repo_dir):
    """List all source files in the repo."""
    extensions = {'.rs', '.py', '.wgsl', '.toml'}
    files = []
    for f in repo_dir.rglob('*'):
        if f.suffix in extensions and 'target' not in str(f) and 'node_modules' not in str(f) and '.venv' not in str(f):
            files.append(f)
    return sorted(files)

def fetch_github_file_list(repo):
    """Get file tree from GitHub API."""
    try:
        url = f"https://api.github.com/repos/{repo}/git/trees/main?recursive=1"
        req = urllib.request.Request(url, headers={"User-Agent": "CESAROPS"})
        resp = json.loads(urllib.request.urlopen(req, timeout=15).read())
        return [f["path"] for f in resp.get("tree", []) if f["type"] == "blob" and (f["path"].endswith('.rs') or f["path"].endswith('.py') or f["path"].endswith('.wgsl'))]
    except Exception as e:
        print(f"  GitHub error for {repo}: {e}")
        return []

def fetch_github_file(repo, path):
    """Fetch a file from GitHub."""
    try:
        url = f"https://raw.githubusercontent.com/{repo}/main/{path}"
        req = urllib.request.Request(url, headers={"User-Agent": "CESAROPS"})
        return urllib.request.urlopen(req, timeout=15).read().decode('utf-8', errors='replace')
    except:
        return ""

# ═══════════════════════════════════════════════════════════════════════════════
print("=" * 70)
print("CESAROPS DEEP ANALYSIS v2 — File-by-File + Triple-Lock Implementation")
print("=" * 70)

# Phase 1: Inventory all source files
print("\n=== PHASE 1: Source File Inventory ===")
local_files = list_source_files(REPO_DIR)
print(f"Local repo: {len(local_files)} source files")

github_files = {}
for repo in GITHUB_REPOS:
    files = fetch_github_file_list(repo)
    github_files[repo] = files
    print(f"GitHub {repo}: {len(files)} source files")

# Phase 2: Deep scan key files with DeepSeek-R1 (fast COT)
print("\n=== PHASE 2: Deep File Analysis (DeepSeek-R1 COT) ===")

# Key files to analyze in detail
KEY_PATTERNS = [
    "pipeline.rs", "synthetic_grid.rs", "tile_store.rs",
    "spatial_engine.rs", "fdct_kernels.rs", "cluster.rs",
    "research_engine.rs", "steering.rs", "weather_service.py",
    "mission_control.py", "universal_downloader.py", "scan_worker.py",
]

analyses = []
for pattern in KEY_PATTERNS:
    # Find in local repo
    matches = [f for f in local_files if f.name == pattern]
    if not matches:
        # Try GitHub
        for repo, files in github_files.items():
            gh_matches = [f for f in files if f.endswith(pattern)]
            if gh_matches:
                content = fetch_github_file(repo, gh_matches[0])
                if content:
                    print(f"  [{pattern}] from GitHub {repo} ({len(content)} chars)")
                    # Analyze with DeepSeek-R1
                    analysis = call_llm(
                        DEEPSEEK,
                        "You are analyzing source code for a shipwreck detection system. Focus on: what does this file do, what's working, what's stubbed/missing, and how does it relate to thermal detection, drift correction, and plume detection.",
                        f"File: {gh_matches[0]}\n\n```\n{content[:8000]}\n```\n\nAnalyze this file. What works? What's missing? How does it handle thermal sinks, drift, and plumes?",
                        max_tokens=2048
                    )
                    analyses.append({"file": f"{repo}/{gh_matches[0]}", "analysis": analysis})
                    with open(OUTPUT_DIR / f"analysis_{pattern.replace('.', '_')}.md", "w") as f:
                        f.write(f"# {pattern}\n\nSource: {repo}/{gh_matches[0]}\n\n{analysis}\n")
                break
    else:
        content = matches[0].read_text(errors='replace')
        print(f"  [{pattern}] local ({len(content)} chars)")
        analysis = call_llm(
            DEEPSEEK,
            "You are analyzing source code for a shipwreck detection system. Focus on: what does this file do, what's working, what's stubbed/missing, and how does it relate to thermal detection, drift correction, and plume detection.",
            f"File: {matches[0].relative_to(REPO_DIR)}\n\n```\n{content[:8000]}\n```\n\nAnalyze this file. What works? What's missing? How does it handle thermal sinks, drift, and plumes?",
            max_tokens=2048
        )
        analyses.append({"file": str(matches[0].relative_to(REPO_DIR)), "analysis": analysis})
        with open(OUTPUT_DIR / f"analysis_{pattern.replace('.', '_')}.md", "w") as f:
            f.write(f"# {pattern}\n\nSource: {matches[0].relative_to(REPO_DIR)}\n\n{analysis}\n")

print(f"\nAnalyzed {len(analyses)} key files")

# Phase 3: Have the 35B write the Triple-Lock Rust implementation
print("\n=== PHASE 3: Triple-Lock Implementation (Qwen3.6-35B on P100s) ===")

# Gather context from nautivecs
context_parts = []
for q in ["detection pipeline process tile anomaly", "sovereign cloud dispatch node", "synthetic tile stack anomaly delta"]:
    ctx = query_nautivecs(q, top_k=3)
    if ctx:
        context_parts.append(ctx)

combined_analyses = "\n\n".join([f"### {a['file']}\n{a['analysis'][:500]}" for a in analyses[:6]])

impl_prompt = f"""Based on the deep codebase analysis and the Triple-Lock architecture, write the COMPLETE Rust implementation.

{TRIPLE_LOCK}

## Codebase Context (from nautivecs + file analysis):
{'\n'.join(context_parts[:2])}

## Key Findings from Analysis:
{combined_analyses}

## Write these files:

=== FILE: cesarops-detection/Cargo.toml ===
[dependencies: reqwest, tokio, serde, serde_json, anyhow, tracing]

=== FILE: cesarops-detection/src/lib.rs ===
[DetectionPipeline, Node trait, Confidence enum, MissionAction enum]

=== FILE: cesarops-detection/src/scout.rs ===
[Node1060 - Florence-2 client, sends tile patches, receives anomaly reports]

=== FILE: cesarops-detection/src/validator.rs ===
[NodeP1000 - Moondream2 client, independent visual confirmation]

=== FILE: cesarops-detection/src/jitter.rs ===
[NodeTPU - HTTP client to TPU VM, sends thermal timeseries, receives material classification]

=== FILE: cesarops-detection/src/reasoner.rs ===
[Node1070 - DeepSeek-R1 client, takes all three reports, produces final decision]

=== FILE: cesarops-detection/src/types.rs ===
[GeoTile, ScoutReport, ValidationReport, JitterSignature, MissionAction structs]

Follow the Rust codegen corrections:
- Enum dispatch not trait objects for async
- Compute values before moving into structs
- Vec<(&str, &str)> for HTTP params
- Annotate .collect() types
"""

print("Sending to Qwen3.6-35B for implementation...")
start = time.time()
implementation = call_llm(QWEN35B, "You are CESAROPS. Write production Rust code.", impl_prompt, max_tokens=16384, temperature=0.3)
elapsed = time.time() - start
print(f"Generated: {len(implementation)} chars in {elapsed:.0f}s")

with open(OUTPUT_DIR / "triple_lock_implementation.md", "w") as f:
    f.write(f"# Triple-Lock Detection Pipeline — Rust Implementation\n\n{implementation}\n")

# Parse and write files
import re
file_pattern = r'=== FILE: (.+?) ==='
parts = re.split(file_pattern, implementation)
files_written = 0
if len(parts) > 1:
    for i in range(1, len(parts), 2):
        filename = parts[i].strip()
        file_content = parts[i+1].strip() if i+1 < len(parts) else ""
        file_content = re.sub(r'^```\w*\n?', '', file_content)
        file_content = re.sub(r'\n?```\s*$', '', file_content)
        file_content = file_content.strip()
        
        filepath = REPO_DIR.parent / filename
        filepath.parent.mkdir(parents=True, exist_ok=True)
        filepath.write_text(file_content + "\n")
        print(f"  Written: {filename} ({len(file_content)} bytes)")
        files_written += 1

print(f"\nTotal implementation files: {files_written}")

# Phase 4: Summary report
print("\n=== PHASE 4: Writing Summary ===")
with open(OUTPUT_DIR / "SUMMARY.md", "w") as f:
    f.write("# CESAROPS Deep Analysis v2 — Summary\n\n")
    f.write(f"Date: {time.strftime('%Y-%m-%d %H:%M UTC')}\n")
    f.write(f"Files analyzed: {len(analyses)}\n")
    f.write(f"Implementation files written: {files_written}\n\n")
    f.write("## Key Findings\n\n")
    for a in analyses:
        f.write(f"### {a['file']}\n{a['analysis'][:300]}...\n\n")
    f.write("\n## Next Steps\n")
    f.write("1. Build cesarops-detection crate\n")
    f.write("2. Deploy Florence-2 on cesarops3 (1060)\n")
    f.write("3. Deploy Moondream2 on cesarops2 (P1000)\n")
    f.write("4. Reboot T440 for IOMMU, start TPU VM\n")
    f.write("5. Wire Mission Control to show triple-lock status lights\n")

print(f"\nDone. Results in: {OUTPUT_DIR}")
print(f"Total time: {time.time() - time.time():.0f}s")
