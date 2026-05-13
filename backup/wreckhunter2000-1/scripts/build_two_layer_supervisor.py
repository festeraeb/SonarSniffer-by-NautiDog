#!/usr/bin/env python3
"""
Two-layer supervisor architecture:
Layer 1: cesarops-bootstrap (tiny, dumb, reliable — gets LLM running)
Layer 2: cesarops-agent (smart, uses LLM to monitor and manage the cluster)

Bootstrap starts first, gets the LLM online, then spawns the agent.
If the agent dies, bootstrap restarts it. If LLM dies, bootstrap restarts it.
Bootstrap is the ONE thing that must never fail.
"""
import json
import urllib.request
import time
import subprocess
from pathlib import Path

KOBOLD = "http://localhost:5001/v1"
REPO = Path("/home/cesarops/wreckhunter2000-1")
OUTPUT = Path("/mnt/data-external/cesarops/analysis-v2")

def call_35b(system, user, max_tokens=16384):
    payload = json.dumps({
        "model": "auto",
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "max_tokens": max_tokens, "temperature": 0.3, "stream": False
    }).encode()
    req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    resp = json.loads(urllib.request.urlopen(req, timeout=1200).read())
    return resp["choices"][0]["message"]["content"]

def parse_and_write(content, base_dir):
    import re
    parts = re.split(r'=== FILE: (.+?) ===', content)
    written = 0
    if len(parts) > 1:
        for i in range(1, len(parts), 2):
            filename = parts[i].strip()
            file_content = parts[i+1].strip() if i+1 < len(parts) else ""
            file_content = re.sub(r'^```\w*\n?', '', file_content)
            file_content = re.sub(r'\n?```\s*$', '', file_content)
            file_content = file_content.strip()
            filepath = base_dir / filename
            filepath.parent.mkdir(parents=True, exist_ok=True)
            filepath.write_text(file_content + "\n")
            print(f"  Written: {filename} ({len(file_content)} bytes)")
            written += 1
    return written

# ═══════════════════════════════════════════════════════════════════════════════
# LAYER 1: BOOTSTRAP
# ═══════════════════════════════════════════════════════════════════════════════
print("=" * 60)
print("LAYER 1: cesarops-bootstrap (dumb, reliable)")
print("=" * 60)

bootstrap_result = call_35b(
    """You are writing a MINIMAL, BULLETPROOF Rust binary. No panics. No unwraps. 
Every error is handled gracefully. This binary must NEVER crash.
RUST CODEGEN RULES: enum dispatch, compute before move, annotate collect types.""",
    """Write `cesarops-bootstrap` — the Layer 1 supervisor.

This is TINY and DUMB. It does exactly this on startup:
1. Mount drives (check if mounted, mount if not, continue if mount fails)
2. Start cloudflared (check if running, start if not)
3. Start tailscaled (check if running, start if not)  
4. Start smbd (check if running, start if not)
5. Start KoboldCPP with the model (check if port 5001 responds, start if not)
6. Wait for KoboldCPP to be healthy (poll /v1/models every 5s, timeout 120s)
7. Start the Layer 2 agent (cesarops-agent binary)
8. Enter watchdog loop: every 30s check KoboldCPP and agent are alive, restart if dead

It does NOT use the LLM for decisions. It's pure if/else logic.
It reads a simple TOML config for paths and ports.

Output:
=== FILE: cesarops-bootstrap/Cargo.toml ===
[minimal deps: tokio, toml, serde, reqwest, tracing]

=== FILE: cesarops-bootstrap/src/main.rs ===
[THE ENTIRE BINARY IN ONE FILE — keep it under 300 lines, simple, readable]

=== FILE: cesarops-bootstrap/bootstrap.toml ===
[Config: paths to binaries, mount points, ports to check]

Keep it SIMPLE. No abstractions. No traits. Just functions that do one thing.
Every system call wrapped in match with error logging and continue."""
)

(OUTPUT / "bootstrap_impl.md").write_text(f"# Bootstrap\n\n{bootstrap_result}\n")
written = parse_and_write(bootstrap_result, REPO)
print(f"Bootstrap files: {written}")

# ═══════════════════════════════════════════════════════════════════════════════
# LAYER 2: AGENTIC SUPERVISOR
# ═══════════════════════════════════════════════════════════════════════════════
print("\n" + "=" * 60)
print("LAYER 2: cesarops-agent (smart, LLM-powered)")
print("=" * 60)

agent_result = call_35b(
    """You are writing an AGENTIC Rust binary that uses a local LLM to make decisions.
It's like n8n but in Rust — workflow nodes that trigger based on conditions.
The LLM (at localhost:5001) helps it decide what to do when things go wrong.
RUST CODEGEN RULES: enum dispatch, compute before move, annotate collect types.""",
    """Write `cesarops-agent` — the Layer 2 intelligent supervisor.

This is the SMART layer. It has access to the LLM and makes decisions:

WORKFLOW NODES (like n8n):
1. HealthMonitor — polls all services every 30s, tracks uptime history
2. WeatherWatcher — checks NOAA buoy data, triggers scan scheduling
3. ModelManager — decides which model to load based on current task queue
4. DriftMonitor — watches for new tiles, triggers drift correction on Xeons
5. AlertManager — sends notifications via Mission Control WebSocket
6. SelfHealer — when a service is unhealthy, asks the LLM what to do

AGENTIC BEHAVIOR:
- When a service dies, it doesn't just restart blindly
- It asks the LLM: "KoboldCPP crashed with OOM. GPU memory shows 30GB used. What should I do?"
- The LLM responds: "The model is too large. Swap to the 14B model and restart."
- The agent executes the LLM's decision

WORKFLOW ENGINE:
- Nodes are connected in a DAG (like n8n)
- Each node has: trigger condition, action, fallback
- Nodes can pass data to each other
- The workflow is defined in a TOML file

HTTP API (port 9000):
- GET /status — all service health + workflow state
- GET /workflows — list active workflows
- POST /trigger/{workflow} — manually trigger a workflow
- GET /logs — recent decisions and actions
- WebSocket /ws — real-time status updates for the dashboard

Output:
=== FILE: cesarops-agent/Cargo.toml ===
[deps: tokio, axum, reqwest, serde, toml, tracing, chrono, tower-http]

=== FILE: cesarops-agent/src/main.rs ===
[entry point — loads workflows, starts monitoring loop, serves API]

=== FILE: cesarops-agent/src/nodes.rs ===
[Workflow nodes: HealthMonitor, WeatherWatcher, ModelManager, etc.]

=== FILE: cesarops-agent/src/workflow.rs ===
[Workflow engine: DAG execution, node triggering, data passing]

=== FILE: cesarops-agent/src/llm.rs ===
[LLM client for decision-making — asks the 35B what to do]

=== FILE: cesarops-agent/src/dashboard.rs ===
[HTML dashboard with real-time status, workflow visualization]

=== FILE: cesarops-agent/workflows.toml ===
[Default workflow definitions]

This is the brain of the cluster. It uses the LLM to make intelligent decisions
about resource allocation, model loading, scan scheduling, and self-healing."""
)

(OUTPUT / "agent_impl.md").write_text(f"# Agent\n\n{agent_result}\n")
written2 = parse_and_write(agent_result, REPO)
print(f"Agent files: {written2}")

# ═══════════════════════════════════════════════════════════════════════════════
# BUILD BOTH
# ═══════════════════════════════════════════════════════════════════════════════
print("\n" + "=" * 60)
print("BUILDING")
print("=" * 60)

# Add to workspace
ws_path = REPO / "Cargo.toml"
ws = ws_path.read_text()
for crate in ["cesarops-bootstrap", "cesarops-agent"]:
    if crate not in ws:
        ws = ws.replace('"cesarops-detection"', f'"cesarops-detection", "{crate}"')
ws_path.write_text(ws)

for crate in ["cesarops-bootstrap", "cesarops-agent"]:
    print(f"\nBuilding {crate}...")
    result = subprocess.run(
        ["bash", "-c", f"source ~/.cargo/env && cd {REPO} && cargo build --release -p {crate} 2>&1 | tail -10"],
        capture_output=True, text=True, timeout=300
    )
    print(result.stdout)
    if "Finished" in result.stdout:
        print(f"  {crate}: BUILD SUCCESS")
    else:
        print(f"  {crate}: BUILD FAILED")

print("\nDone.")
