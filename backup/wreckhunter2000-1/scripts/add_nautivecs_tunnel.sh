#!/bin/bash
# Add nautivecs search API to cloudflare tunnel and run whitepaper through the full pipeline
set -euo pipefail

SUDO_PASS="cesarops"
run_sudo() { echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null; }

echo "=== Adding search.cesarops.org to tunnel config ==="

# Find the config file
CONFIG=$(find /etc/cloudflared /home/cesarops/.cloudflared -name 'config.yml' 2>/dev/null | head -1)
if [ -z "$CONFIG" ]; then
    echo "ERROR: No cloudflared config found"
    exit 1
fi
echo "Config: $CONFIG"

# Add nautivecs route if not present
if ! grep -q "search.cesarops.org" "$CONFIG"; then
    # Insert before the catch-all rule
    run_sudo sed -i '/service: http_status:404/i\  - hostname: search.cesarops.org\n    service: http://localhost:5003' "$CONFIG"
    echo "Added search.cesarops.org -> localhost:5003"
    
    # Restart tunnel
    run_sudo systemctl restart cloudflared
    sleep 2
    echo "Tunnel restarted"
else
    echo "Already configured"
fi

# Add DNS route
TUNNEL_ID=$(grep '^tunnel:' "$CONFIG" | awk '{print $2}')
if [ -n "$TUNNEL_ID" ]; then
    cloudflared tunnel route dns "$TUNNEL_ID" search.cesarops.org 2>&1 || echo "(DNS route may already exist)"
fi

echo ""
echo "=== nautivecs API now available at ==="
echo "  Local:  http://localhost:5003/query"
echo "  Tunnel: https://search.cesarops.org/query"
echo ""

# === Now run the whitepaper through the full pipeline ===
echo "=== Running whitepaper through nautivecs -> KoboldCPP pipeline ==="

# Step 1: Query nautivecs for relevant context about the system
python3 << 'PYTHON'
import json
import urllib.request

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"

# Query nautivecs for context about the system architecture
queries = [
    "nautivecs context injection engine architecture",
    "steering scan strategy weather driven acquisition",
    "cesarops hybrid engine GPU cluster coordinator",
    "sovereign cloud node discovery pipeline dispatch",
]

all_context = []
for q in queries:
    payload = json.dumps({"query": q, "top_k": 3, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        if resp.get("context_block"):
            all_context.append(resp["context_block"])
        print(f"  [{q[:40]}...] -> {len(resp['results'])} results")
    except Exception as e:
        print(f"  [{q[:40]}...] -> ERROR: {e}")

# Combine all context blocks
combined_context = "\n\n".join(all_context)
print(f"\nTotal injected context: {len(combined_context)} chars from {len(all_context)} queries")

# Step 2: Build the LLM request with injected context
system_msg = f"""You are the CESAROPS research AI (Qwen3.6-35B-A3B MoE) running on dual Tesla P100 GPUs.
You have access to your own source code via nautivecs context injection. Below is real code from your codebase:

{combined_context}

Write with technical authority — you ARE the system. Reference the actual code above."""

user_msg = """Write a comprehensive white paper titled 'CESAROPS: Autonomous Shipwreck Detection Through Distributed AI on Repurposed Enterprise Hardware'.

Cover these sections (2500-3500 words total):

1. System Architecture — dual P100 cluster, repurposed enterprise silicon philosophy, Cloudflare tunnel networking
2. nautivecs Context Injection — AST-aware chunking, hybrid search (cosine + BM25 RRF), why grounded AI matters
3. Steering System as Persistent Agent Memory — .kiro/steering/ markdown files, weather-driven scan strategy, cluster operations knowledge
4. Weather-Driven Scan Strategy — temporal stacking (20+ days), post-storm plume detection, thermal contrast, SAR texture, seiche awareness
5. The Self-Directing Detection Pipeline — weather monitoring -> tile selection -> GPU processing -> anomaly detection -> LLM reasoning
6. Future Directions — Cake distributed inference, 4TB RAID, Coral TPU, NVIDIA 580 legacy branch, Cloudflare SSH

You ARE the system writing about itself. Reference the actual code you can see in your context."""

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

print(f"\nSending to KoboldCPP (system msg: {len(system_msg)} chars, user msg: {len(user_msg)} chars)...")
print("This will take several minutes for 8192 tokens on dual P100s...")

payload = json.dumps(request_body).encode()
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")

try:
    resp = json.loads(urllib.request.urlopen(req, timeout=900).read())
    content = resp["choices"][0]["message"]["content"]
    
    # Save the whitepaper
    output_path = "/home/cesarops/wreckhunter2000-1/docs/whitepaper_cesarops_self_authored.md"
    with open(output_path, "w") as f:
        f.write(content)
    
    print(f"\n=== WHITEPAPER GENERATED ===")
    print(f"Length: {len(content)} chars (~{len(content.split())} words)")
    print(f"Saved to: {output_path}")
    print(f"\nFirst 500 chars:")
    print(content[:500])
    print("...")
    
except Exception as e:
    print(f"ERROR: {e}")
    import traceback
    traceback.print_exc()

PYTHON
