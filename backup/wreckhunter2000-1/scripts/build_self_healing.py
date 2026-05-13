#!/usr/bin/env python3
"""
Build the self-healing two-layer supervisor using the correct architecture:
- Layer 1: Dumb recovery binary (self-replace, watchdog, process manager)
- Layer 2: Rust n8n agent (LLM-powered orchestration, workflow nodes)
- Self-annealing loop: detect → diagnose → remediate
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
# LAYER 1: DUMB RECOVERY BINARY
# ═══════════════════════════════════════════════════════════════════════════════
print("=" * 60)
print("LAYER 1: cesarops-watchdog (dumb, self-replacing, bulletproof)")
print("=" * 60)

layer1 = call_35b(
    """You are writing a MINIMAL Rust process supervisor. Use these patterns:
- self-replace crate: allows the binary to hot-swap itself on disk
- Watchdog pattern: if a managed process hangs, kill it after timeout
- HTTP health webhook: sends "I'm alive" pings to the agent layer
- Process table: tracks PIDs, restart counts, backoff timers

NO LLM calls. NO complex logic. Just: start processes, watch them, restart if dead.
RUST CODEGEN RULES: No unwraps. No panics. Every error logged and continued.""",
    """Write `cesarops-watchdog` — the Layer 1 dumb recovery binary.

Crates to use:
- self-replace (for hot-swapping the binary itself)
- tokio (async runtime)
- serde + toml (config)
- reqwest (health pings)
- tracing (structured logging)

What it does:
1. Reads watchdog.toml for process definitions
2. For each process: spawn it, track PID, monitor health
3. Health check: either HTTP endpoint poll OR "process is alive" (kill -0)
4. If unhealthy: kill, wait backoff, restart (backoff: 5s → 15s → 60s → 300s)
5. Sends heartbeat to Layer 2 agent every 10s: POST http://localhost:9000/heartbeat
6. Exposes GET http://localhost:9001/status (simple JSON of all process states)
7. Listens for POST http://localhost:9001/replace with a new binary path → uses self-replace to swap itself
8. Listens for POST http://localhost:9001/restart/{name} to force-restart a process

Process definitions in config:
- name, binary path, args, working dir, health check (url or "pid"), restart policy
- Startup order (sequential groups — mounts first, then network, then services)

Startup groups:
1. MOUNTS: check/mount drives (shell commands, not processes)
2. NETWORK: cloudflared, tailscaled
3. INFRA: smbd, code-server
4. LLM: koboldcpp (wait for healthy before proceeding)
5. APPS: nautivecs-server, mission-control, cesarops-detection
6. AGENT: cesarops-agent (Layer 2)

Output:
=== FILE: cesarops-watchdog/Cargo.toml ===
=== FILE: cesarops-watchdog/src/main.rs ===
[ENTIRE binary in one file, under 400 lines]
=== FILE: cesarops-watchdog/watchdog.toml ===
[Full config with all CESAROPS services]"""
)

(OUTPUT / "watchdog_impl.md").write_text(f"# Watchdog (Layer 1)\n\n{layer1}\n")
w1 = parse_and_write(layer1, REPO)
print(f"Watchdog files: {w1}")

# ═══════════════════════════════════════════════════════════════════════════════
# LAYER 2: RUST N8N AGENT
# ═══════════════════════════════════════════════════════════════════════════════
print("\n" + "=" * 60)
print("LAYER 2: cesarops-agent (Rust n8n, LLM-powered self-healing)")
print("=" * 60)

layer2 = call_35b(
    """You are writing a Rust-based n8n equivalent — an agentic workflow engine.
It uses a local LLM (OpenAI-compatible at localhost:5001) to make decisions.
It monitors the Layer 1 watchdog and all cluster services.
RUST CODEGEN RULES: enum dispatch, compute before move, annotate collect types.""",
    """Write `cesarops-agent` — the Layer 2 intelligent orchestrator.

This is a Rust n8n replacement with LLM-powered decision making.

ARCHITECTURE:
- Workflow engine: nodes connected in a DAG, triggered by conditions
- Each node: trigger → action → output (passed to next node)
- LLM integration: any node can ask the LLM for a decision
- Self-annealing: detect failure → diagnose with LLM → remediate

WORKFLOW NODES (built-in):
1. HealthCheck — polls a URL, fires if unhealthy
2. Timer — fires on schedule (cron-like)
3. LLMDecision — sends context to LLM, parses response as action
4. ProcessControl — restart/stop/start a process via watchdog API
5. ModelSwap — tells watchdog to load a different model
6. Alert — sends notification to Mission Control WebSocket
7. ShellExec — runs a shell command, captures output
8. HttpRequest — makes an HTTP call, passes response to next node
9. Condition — if/else branching based on data
10. SelfReplace — triggers watchdog binary hot-swap

SELF-ANNEALING LOOP:
```
HealthCheck(koboldcpp) → [unhealthy] → LLMDecision("KoboldCPP is down, GPU shows X. What do?")
  → LLM says "OOM, swap to smaller model"
  → ModelSwap(coder14)
  → ProcessControl(restart, koboldcpp)
  → HealthCheck(koboldcpp) → [healthy] → Alert("Recovered: swapped to 14B model")
```

HTTP API (port 9000):
- GET /status — cluster health overview
- GET /workflows — list all workflows and their state
- POST /workflows/{id}/trigger — manually trigger
- GET /decisions — recent LLM decisions with reasoning
- WebSocket /ws — real-time events for dashboard

DASHBOARD (embedded HTML at GET /):
- Visual workflow graph (nodes + connections)
- Service status lights (green/yellow/red)
- Recent decisions log
- Manual trigger buttons

Output:
=== FILE: cesarops-agent/Cargo.toml ===
=== FILE: cesarops-agent/src/main.rs ===
=== FILE: cesarops-agent/src/workflow.rs ===
[Workflow engine: DAG, node execution, data passing]
=== FILE: cesarops-agent/src/nodes.rs ===
[All built-in node types]
=== FILE: cesarops-agent/src/llm.rs ===
[LLM client for decision-making]
=== FILE: cesarops-agent/src/dashboard.rs ===
[Embedded HTML dashboard with workflow visualization]
=== FILE: cesarops-agent/workflows.toml ===
[Default self-healing workflows]"""
)

(OUTPUT / "agent_impl.md").write_text(f"# Agent (Layer 2)\n\n{layer2}\n")
w2 = parse_and_write(layer2, REPO)
print(f"Agent files: {w2}")

# ═══════════════════════════════════════════════════════════════════════════════
# BUILD
# ═══════════════════════════════════════════════════════════════════════════════
print("\n" + "=" * 60)
print("BUILDING BOTH LAYERS")
print("=" * 60)

ws_path = REPO / "Cargo.toml"
ws = ws_path.read_text()
for crate in ["cesarops-watchdog", "cesarops-agent"]:
    if crate not in ws:
        # Find the last crate in members and append
        import re
        ws = re.sub(r'(members = \[.*?)"(\])', rf'\1", "{crate}"\2', ws)
ws_path.write_text(ws)

for crate in ["cesarops-watchdog", "cesarops-agent"]:
    print(f"\nBuilding {crate}...")
    result = subprocess.run(
        ["bash", "-c", f"source ~/.cargo/env && cd {REPO} && cargo build --release -p {crate} 2>&1 | tail -10"],
        capture_output=True, text=True, timeout=300
    )
    output = result.stdout + result.stderr
    print(output[-500:] if len(output) > 500 else output)
    if "Finished" in output:
        print(f"  ✓ {crate}: BUILD SUCCESS")
    else:
        errors = [l for l in output.split('\n') if 'error[' in l]
        print(f"  ✗ {crate}: {len(errors)} errors — needs fixes")

print("\n" + "=" * 60)
print("DONE")
print("=" * 60)
