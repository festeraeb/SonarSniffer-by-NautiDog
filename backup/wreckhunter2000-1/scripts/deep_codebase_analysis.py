#!/usr/bin/env python3
"""
Deep Codebase Analysis — runs on cesarops2 via the Thought Engine.
Uses DeepSeek-R1 (COT) + nautivecs + GitHub repos to produce:
1. Strengths of the current system
2. Fixes needed
3. Things to add to nautivecs knowledge base
4. Historical context from old repos/branches

Runs in chunks to stay within context limits.
"""
import json
import time
import urllib.request
import os

# The thought engine on cesarops2
THOUGHT_ENGINE = "http://localhost:5556"
# Direct to DeepSeek-R1 for longer analysis (bypass thought engine overhead)
DEEPSEEK = "http://localhost:5555/v1"
# nautivecs on T440
NAUTIVECS = "http://100.72.182.77:5003"
# Output
OUTPUT_DIR = "/home/cesarops"
RESULTS = []

def query_nautivecs(query, top_k=5):
    """Search the codebase via nautivecs."""
    payload = json.dumps({"query": query, "top_k": top_k, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        return resp.get("context_block", ""), resp.get("results", [])
    except Exception as e:
        print(f"  nautivecs error: {e}")
        return "", []

def ask_deepseek(system_prompt, user_prompt, max_tokens=4096, temperature=0.6):
    """Ask DeepSeek-R1 directly for COT reasoning."""
    payload = json.dumps({
        "model": "deepseek-r1",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ],
        "max_tokens": max_tokens,
        "temperature": temperature,
        "stream": False
    }).encode()
    req = urllib.request.Request(f"{DEEPSEEK}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=300).read())
        return resp["choices"][0]["message"]["content"]
    except Exception as e:
        print(f"  DeepSeek error: {e}")
        return f"ERROR: {e}"

def list_github_repos():
    """List repos under festeraeb on GitHub."""
    try:
        req = urllib.request.Request("https://api.github.com/users/festeraeb/repos?per_page=100&sort=updated", headers={"User-Agent": "CESAROPS"})
        resp = json.loads(urllib.request.urlopen(req, timeout=15).read())
        return [(r["name"], r["description"] or "", r["updated_at"], r["default_branch"]) for r in resp]
    except Exception as e:
        print(f"  GitHub error: {e}")
        return []

# ═══════════════════════════════════════════════════════════════════════════════
# ANALYSIS CHUNKS
# ═══════════════════════════════════════════════════════════════════════════════

SYSTEM_PROMPT = """You are DeepSeek-R1, a chain-of-thought reasoning model running on a GTX 1070 as part of the CESAROPS shipwreck detection cluster.

Your job: Analyze code and produce actionable findings. Think step by step.
Focus on:
- STRENGTHS: What's well-designed, what works, what's clever
- FIXES NEEDED: Bugs, stubs, missing implementations, broken paths
- KNOWLEDGE TO ADD: Things that should be indexed in nautivecs for future reference
- THERMAL SINKS: Steel hulls absorb/release heat differently — any code handling this?
- DRIFT CORRECTION: Sub-pixel alignment between satellite passes — the core unsolved problem
- POST-STORM PLUMES: Sediment disruption detection — the primary detection method
- WEATHER INTEGRATION: How weather data feeds into the pipeline

Be brutally honest. Cite specific files and functions."""

ANALYSIS_CHUNKS = [
    {
        "name": "Detection Pipeline",
        "queries": [
            "detect glint hydrocarbon thermal anomaly pipeline",
            "synthetic tile anomaly delta stack weight",
            "curvelet transform edge detection nauticuvs",
        ],
        "question": "Analyze the detection pipeline. What actually works for finding wrecks? What's stubbed? How does thermal sink detection work (steel hulls as heat anomalies)? How does post-storm plume detection work? What's the path from raw tile to wreck candidate?"
    },
    {
        "name": "Drift Correction & Alignment",
        "queries": [
            "drift correction sub pixel slice stitch align",
            "phase correlation registration reference frame",
            "tile stack temporal alignment coordinate",
        ],
        "question": "Analyze the drift correction approach. Is there actual slice-stitch-replace code? How does sub-pixel alignment work? What uses the Xeon AVX-512? Is ndarray or SIMD used? What's the current state — working, stubbed, or missing? How many pixels of drift is acceptable?"
    },
    {
        "name": "Weather & Sensor Integration",
        "queries": [
            "weather service NOAA buoy wind storm calm",
            "scan strategy post storm plume thermal contrast",
            "sensor satellite download HLS Sentinel ICESat SWOT",
        ],
        "question": "Analyze weather integration. How does weather data drive scan decisions? Is the steering file (scan-strategy.md) actually used in code or just documentation? How do thermal contrast days get detected? How does the system know when a storm ended for plume detection? What sensors are actually downloading data vs just listed?"
    },
    {
        "name": "Cluster Orchestration & Model Management",
        "queries": [
            "sovereign cloud node discovery pipeline dispatch",
            "koboldcpp model swap service GPU memory",
            "thought engine process task dispatch verify",
        ],
        "question": "Analyze the cluster orchestration. How do models get loaded/unloaded on the P100s? How does the thought engine coordinate with the 35B? Is there actual GPU mode-flipping (LLM vs compute)? What happens when the P100s need to switch from inference to scan processing?"
    },
    {
        "name": "Data Pipeline & Storage",
        "queries": [
            "universal downloader satellite tile download area",
            "tile store sled JSON persistence region query",
            "scan worker queue job process upload",
        ],
        "question": "Analyze the data pipeline. How do tiles get from satellite APIs to GPU processing? What's the storage format? How does the queue work? Is there actual end-to-end flow from download to detection to report? What's missing in the chain?"
    },
    {
        "name": "AI Grounding & Context",
        "queries": [
            "nautivecs context injection AST chunk search",
            "steering corrections feedback human loop",
            "research engine synthesis findings web oracle",
        ],
        "question": "Analyze the AI grounding system. How does nautivecs prevent hallucination? How do steering files persist knowledge? How does the research engine synthesize findings? Is the web search oracle actually connected? What knowledge should be added to nautivecs that isn't there?"
    },
]

# ═══════════════════════════════════════════════════════════════════════════════
# EXECUTION
# ═══════════════════════════════════════════════════════════════════════════════

print("=" * 70)
print("CESAROPS DEEP CODEBASE ANALYSIS")
print(f"Model: DeepSeek-R1-Distill-Llama-8B (COT) on GTX 1070")
print(f"Context: nautivecs (2272 chunks) + GitHub repos")
print("=" * 70)

# First: scan GitHub repos
print("\n=== GITHUB REPOS (festeraeb) ===")
repos = list_github_repos()
repo_summary = ""
for name, desc, updated, branch in repos:
    skip = name.lower() in ["wayfinder", "sonar-sniffer", "garmin-rsd", "garminrsd"]
    marker = " [SKIP]" if skip else ""
    print(f"  {name}: {desc[:50]} (branch: {branch}, updated: {updated[:10]}){marker}")
    if not skip:
        repo_summary += f"- {name}: {desc} (branch: {branch})\n"

# Run each analysis chunk
print("\n" + "=" * 70)
all_findings = []

for i, chunk in enumerate(ANALYSIS_CHUNKS):
    print(f"\n{'='*70}")
    print(f"CHUNK {i+1}/{len(ANALYSIS_CHUNKS)}: {chunk['name']}")
    print(f"{'='*70}")

    # Gather context from nautivecs
    context_parts = []
    for q in chunk["queries"]:
        ctx, results = query_nautivecs(q, top_k=3)
        if ctx:
            context_parts.append(ctx)
        print(f"  nautivecs [{q[:40]}...] -> {len(results)} results")

    combined_context = "\n\n".join(context_parts)
    print(f"  Total context: {len(combined_context)} chars")

    # Ask DeepSeek-R1 to analyze
    user_prompt = f"""CODEBASE CONTEXT (from nautivecs search):
{combined_context[:12000]}

GITHUB REPOS (festeraeb):
{repo_summary}

ANALYSIS TASK:
{chunk['question']}

Produce your analysis as:
## Strengths
- [list what works well]

## Fixes Needed
- [list specific bugs/stubs/missing code with file paths]

## Knowledge to Add to nautivecs
- [list facts/patterns that should be indexed for future reference]

## Thermal Sink / Plume Detection Status
- [specifically address heat anomaly and sediment plume detection]

Think step by step. Be specific. Cite files."""

    print(f"  Asking DeepSeek-R1 (COT)...")
    start = time.time()
    result = ask_deepseek(SYSTEM_PROMPT, user_prompt, max_tokens=4096)
    elapsed = time.time() - start
    print(f"  Response: {len(result)} chars in {elapsed:.1f}s")

    all_findings.append({
        "chunk": chunk["name"],
        "analysis": result,
        "elapsed_seconds": elapsed,
    })

    # Save incrementally
    with open(f"{OUTPUT_DIR}/deep_analysis_chunk_{i+1}.md", "w") as f:
        f.write(f"# Analysis: {chunk['name']}\n\n{result}\n")
    print(f"  Saved: deep_analysis_chunk_{i+1}.md")

# Final combined report
print(f"\n{'='*70}")
print("WRITING COMBINED REPORT")
print(f"{'='*70}")

with open(f"{OUTPUT_DIR}/deep_analysis_full.md", "w") as f:
    f.write("# CESAROPS Deep Codebase Analysis\n\n")
    f.write(f"**Model:** DeepSeek-R1-Distill-Llama-8B (COT reasoning)\n")
    f.write(f"**Date:** {time.strftime('%Y-%m-%d %H:%M UTC')}\n")
    f.write(f"**Chunks analyzed:** {len(all_findings)}\n")
    f.write(f"**nautivecs index:** 2272 chunks\n\n")
    f.write(f"## GitHub Repos Examined\n{repo_summary}\n\n")
    f.write("---\n\n")
    for finding in all_findings:
        f.write(f"# {finding['chunk']}\n\n")
        f.write(f"{finding['analysis']}\n\n")
        f.write(f"---\n\n")

print(f"Done. Full report: {OUTPUT_DIR}/deep_analysis_full.md")
print(f"Total chunks: {len(all_findings)}")
total_time = sum(f["elapsed_seconds"] for f in all_findings)
print(f"Total analysis time: {total_time:.0f}s ({total_time/60:.1f} min)")
