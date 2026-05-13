#!/usr/bin/env python3
"""
Ask the 35B to research and spec out Cake distributed inference:
- How to shard across local cluster (T440 + cesarops2 + cesarops3)
- How to add a friend's 2x P100 over Tailscale/internet
- Auto-discovery, topology, and the launcher integration
"""
import json
import urllib.request

KOBOLD = "http://localhost:5001/v1"
NAUTIVECS = "http://localhost:5003"
OUTPUT = "/mnt/data-external/cesarops/analysis-v2/cake_distributed_spec.md"

# Get context about existing Cake work
ctx = ""
for q in ["cake distributed cluster topology shard", "koboldcpp model swap GPU layers"]:
    payload = json.dumps({"query": q, "top_k": 3, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        ctx += resp.get("context_block", "") + "\n\n"
    except:
        pass

payload = json.dumps({
    "model": "auto",
    "messages": [
        {"role": "system", "content": f"You are CESAROPS. Research and spec Cake distributed inference for the cluster.\n\nExisting context:\n{ctx[:8000]}"},
        {"role": "user", "content": """Research and write a complete spec for Cake distributed inference across the CESAROPS cluster.

Hardware available:
- T440: 2x Tesla P100 16GB (32GB total) — primary, Tailscale IP 100.72.182.77
- cesarops2: GTX 1070 8GB + P1000 4GB (12GB total) — Tailscale IP 100.102.158.111
- cesarops3: P106 6GB — Tailscale IP 100.105.77.74
- Friend's machine: 2x Tesla P100 16GB (32GB total) — on Tailscale, 1Gbps internet

Total cluster: 82GB VRAM across 7 GPUs (if friend joins)

Questions to answer:
1. How does Cake's --cluster-key mDNS discovery work? Does it cross subnets via Tailscale?
2. What's the topology for sharding a 70B model across all GPUs?
3. How to handle the friend's P100s over internet (latency impact on layer boundaries)?
4. Can Cake auto-detect VRAM per GPU and assign layers proportionally?
5. What models could we run that DON'T fit on any single machine but DO fit distributed?
6. Write the actual commands to start workers on each node
7. Write a topology.yml for the full cluster
8. Calculate expected tok/s for Llama-3-70B distributed across all 7 GPUs
9. How does this integrate with the launcher script (preset: cake)?
10. Security: is --cluster-key enough or do we need Tailscale ACLs?

Also research: Can Cake do CUDA on some nodes and Vulkan on others in the same cluster?
The P106 mining card needs Vulkan, the P100s use CUDA.

Write a complete, actionable spec with actual commands and configs."""}
    ],
    "max_tokens": 16384,
    "temperature": 0.5,
    "stream": False
}).encode()

print("Asking 35B to research Cake distributed setup...")
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")
resp = json.loads(urllib.request.urlopen(req, timeout=1200).read())
content = resp["choices"][0]["message"]["content"]

with open(OUTPUT, "w") as f:
    f.write(f"# Cake Distributed Inference Spec\n\n{content}\n")

print(f"Done. {len(content)} chars written to {OUTPUT}")
