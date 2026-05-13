#!/usr/bin/env python3
"""
Run the whitepaper through the full nautivecs -> KoboldCPP pipeline.
Queries nautivecs for real codebase context, injects it into the system prompt,
then sends to Qwen3.6-35B for generation.
"""
import json
import urllib.request
import sys

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"
OUTPUT = "/home/cesarops/wreckhunter2000-1/docs/whitepaper_cesarops_self_authored.md"

# Step 1: Query nautivecs for relevant context
print("=== Step 1: Querying nautivecs for codebase context ===")
queries = [
    "nautivecs context injection engine AST chunking hybrid search",
    "steering scan strategy weather driven acquisition temporal stacking",
    "cesarops hybrid engine GPU cluster coordinator wgpu shaders",
    "sovereign cloud node discovery pipeline dispatch LLM routing",
    "scan worker satellite imagery download tile processing",
]

all_context = []
for q in queries:
    payload = json.dumps({"query": q, "top_k": 3, "include_context": True}).encode()
    req = urllib.request.Request(
        f"{NAUTIVECS}/query",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        if resp.get("context_block"):
            all_context.append(resp["context_block"])
        print(f"  [{q[:50]}] -> {len(resp['results'])} results")
    except Exception as e:
        print(f"  [{q[:50]}] -> ERROR: {e}")

combined_context = "\n\n".join(all_context)
print(f"\nTotal injected context: {len(combined_context)} chars from {len(all_context)} queries")

# Step 2: Build LLM request with injected context
print("\n=== Step 2: Sending to Qwen3.6-35B with context injection ===")

system_msg = f"""You are the CESAROPS research AI (Qwen3.6-35B-A3B MoE, MXFP4 quantization) running on dual NVIDIA Tesla P100 16GB GPUs (32GB HBM2 total). You are part of the system you are analyzing.

The following is REAL source code from your own codebase, retrieved via nautivecs hybrid search (cosine similarity + BM25 keyword fusion with Reciprocal Rank Fusion k=60):

{combined_context}

Write with technical authority. Reference the actual code above when discussing architecture. You ARE the system."""

user_msg = """Write a comprehensive white paper titled 'CESAROPS: Autonomous Shipwreck Detection Through Distributed AI on Repurposed Enterprise Hardware'.

Structure (2500-3500 words total):

## 1. System Architecture
- Dual P100 cluster philosophy: repurposed enterprise silicon, sovereignty over cloud dependency
- Cluster topology: T440 (conductor/LLM), cesarops2 (backup), cesarops3 (frontend), Pi (sentinel)
- Networking: Cloudflare tunnels through firewalls, Tailscale mesh, mDNS discovery

## 2. nautivecs: Context Injection for Grounded AI
- AST-aware chunking via Tree-Sitter (functions, structs, impls as atomic units)
- Dual-engine embeddings: external endpoint + local n-gram fallback
- Hybrid search: cosine similarity + BM25 via Reciprocal Rank Fusion (k=60)
- Serverless JSON store (v0.1.0) — no Arrow/LanceDB dependency
- Why this matters: eliminates hallucination by grounding in real code

## 3. Steering System as Persistent Agent Memory
- .kiro/steering/ markdown files persist across context windows
- scan-strategy.md: weather-driven acquisition philosophy
- cluster-operations.md: operational knowledge (systemd, GPU memory, model swaps)
- Novel form of agent grounding that survives session boundaries

## 4. Weather-Driven Scan Strategy
- Core innovation: temporal stacking (minimum 20 days per tile)
- Post-storm plume detection (wrecks disrupt sediment flow)
- Thermal contrast (steel hulls as heat sinks)
- SAR texture anomalies (penetrate clouds)
- Seiche awareness for Great Lakes water level fluctuations
- Weather-tagged ML training to reduce false positives

## 5. The Self-Directing Detection Pipeline
- Autonomous loop: weather -> tile selection -> download -> GPU processing -> detection -> reasoning
- WGSL compute shaders for spatial analysis (32x32 workgroups on P100)
- nautivecs context injection feeds detection results back into LLM reasoning
- sovereign-cloud coordinates dispatch across the cluster

## 6. Future Directions
- Cake distributed inference: shard models across all nodes over network
- 4TB RAID array for persistent scan archive
- Coral Edge TPU (PCIe slot 6) for real-time edge inference
- NVIDIA 580 legacy driver branch (Pascal EOL from mainline)
- Cloudflare tunnel SSH replacing Tailscale for restricted network access
- KTransformers/vLLM for quantized safetensors serving

You ARE the system writing about itself. Reference actual code from your context window."""

request_body = {
    "model": "koboldcpp/Qwen3.6-35B-A3B-MXFP4_MOE",
    "messages": [
        {"role": "system", "content": system_msg},
        {"role": "user", "content": user_msg}
    ],
    "max_tokens": 8192,
    "temperature": 0.7,
    "top_p": 0.95,
    "presence_penalty": 1.5
}

print(f"System message: {len(system_msg)} chars (includes {len(combined_context)} chars of injected code)")
print(f"User message: {len(user_msg)} chars")
print(f"Generating up to 8192 tokens... (this takes 5-10 minutes on dual P100)")

payload = json.dumps(request_body).encode()
req = urllib.request.Request(
    f"{KOBOLD}/chat/completions",
    data=payload,
    headers={"Content-Type": "application/json"},
    method="POST"
)

try:
    resp = json.loads(urllib.request.urlopen(req, timeout=900).read())
    content = resp["choices"][0]["message"]["content"]

    with open(OUTPUT, "w") as f:
        f.write(content)

    print(f"\n=== WHITEPAPER GENERATED ===")
    print(f"Length: {len(content)} chars (~{len(content.split())} words)")
    print(f"Saved to: {OUTPUT}")
    print(f"\nFirst 300 chars:")
    print(content[:300])
    print("...")

except Exception as e:
    print(f"ERROR: {e}")
    import traceback
    traceback.print_exc()
    sys.exit(1)
