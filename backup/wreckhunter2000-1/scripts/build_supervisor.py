#!/usr/bin/env python3
"""
Have the 35B write a Rust supervisor that replaces all systemd services.
One binary to rule them all — starts, monitors, restarts, and falls back.
"""
import json
import urllib.request
import time

KOBOLD = "http://localhost:5001/v1"
OUTPUT = "/mnt/data-external/cesarops/analysis-v2/supervisor_impl.md"

payload = json.dumps({
    "model": "auto",
    "messages": [
        {"role": "system", "content": """You are CESAROPS. Write a Rust supervisor crate that replaces all systemd services.

RUST CODEGEN RULES:
- Enum dispatch not trait objects for async
- Compute values before moving into structs
- Vec<(&str, &str)> for HTTP params
- Annotate .collect() types
- Match arms must return consistent types"""},
        {"role": "user", "content": """Write `cesarops-supervisor` — a single Rust binary that:

1. OWNS all processes (KoboldCPP, nautivecs, mission-control, detection pipeline)
2. MONITORS health every 30 seconds via HTTP health endpoints
3. RESTARTS anything that dies (with backoff: 5s, 15s, 60s, then alert)
4. FALLS BACK gracefully:
   - If nautivecs is down → skip context injection, proceed without
   - If thinking model (cesarops2) is down → route directly to 35B
   - If a GPU node is offline → redistribute work to remaining nodes
   - If KoboldCPP crashes → restart it, queue pending requests
5. LOGS everything to a structured JSON log file
6. EXPOSES a health dashboard at port 9000 (simple HTML showing all service status)
7. STARTS on boot as the ONE systemd service (only one needed)

Services it manages:
- KoboldCPP (local, port 5001) — binary at /home/cesarops/koboldcpp, model at /codebase/models/
- nautivecs-server (local, port 5003) — binary at target/release/nautivecs-cli
- mission-control (local, port 3000) — binary at target/release/cesarops-mission-control
- detection pipeline (local, port 5580) — binary at target/release/cesarops-detection

Remote services it monitors (doesn't own, just checks):
- DeepSeek-R1 on cesarops2 (100.102.158.111:5555)
- Vision models on cesarops3 (100.105.77.74:5572)

Config file: /codebase/supervisor.toml

Output ALL files:

=== FILE: cesarops-supervisor/Cargo.toml ===
[deps: tokio, axum, reqwest, serde, toml, tracing, tracing-subscriber, chrono]

=== FILE: cesarops-supervisor/src/main.rs ===
[entry point — loads config, starts supervisor loop, serves dashboard]

=== FILE: cesarops-supervisor/src/config.rs ===
[TOML config: service definitions, health endpoints, restart policies]

=== FILE: cesarops-supervisor/src/process.rs ===
[Process management: spawn, kill, restart with backoff]

=== FILE: cesarops-supervisor/src/health.rs ===
[Health checking: HTTP polls, timeout handling, status tracking]

=== FILE: cesarops-supervisor/src/fallback.rs ===
[Fallback logic: what to do when each service is down]

=== FILE: cesarops-supervisor/src/dashboard.rs ===
[Simple HTML dashboard showing all service status with colored indicators]

=== FILE: cesarops-supervisor/supervisor.toml ===
[Default config file with all services defined]

Write COMPLETE, COMPILABLE code. This replaces systemd for the entire cluster.
It must be rock solid — no panics, no unwraps on network calls, graceful degradation everywhere."""}
    ],
    "max_tokens": 16384,
    "temperature": 0.3,
    "stream": False
}).encode()

print("Asking 35B to write the supervisor crate...")
start = time.time()
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")
resp = json.loads(urllib.request.urlopen(req, timeout=1200).read())
content = resp["choices"][0]["message"]["content"]
elapsed = time.time() - start

with open(OUTPUT, "w") as f:
    f.write(f"# CESAROPS Supervisor\n\n{content}\n")

print(f"Generated: {len(content)} chars in {elapsed:.0f}s")

# Parse and write files
import re
from pathlib import Path
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
        filepath = Path(f"/home/cesarops/wreckhunter2000-1/{filename}")
        filepath.parent.mkdir(parents=True, exist_ok=True)
        filepath.write_text(file_content + "\n")
        print(f"  Written: {filename} ({len(file_content)} bytes)")
        files_written += 1

print(f"\nFiles: {files_written}")

# Try to build
if files_written > 0:
    import subprocess
    # Add to workspace
    ws_path = Path("/home/cesarops/wreckhunter2000-1/Cargo.toml")
    ws = ws_path.read_text()
    if "cesarops-supervisor" not in ws:
        ws = ws.replace('"cesarops-detection"', '"cesarops-detection", "cesarops-supervisor"')
        ws_path.write_text(ws)
    
    print("\nBuilding...")
    result = subprocess.run(
        ["bash", "-c", "source ~/.cargo/env && cd /home/cesarops/wreckhunter2000-1 && cargo build --release -p cesarops-supervisor 2>&1 | tail -10"],
        capture_output=True, text=True, timeout=300
    )
    print(result.stdout)
    if "Finished" in result.stdout:
        print("BUILD SUCCESS")
    else:
        print("BUILD FAILED — will need fixes")
        # Try one fix pass
        errors = [l for l in result.stdout.split('\n') if 'error[' in l]
        if errors:
            print(f"Errors: {len(errors)}")
