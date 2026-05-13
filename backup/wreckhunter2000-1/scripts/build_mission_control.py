#!/usr/bin/env python3
"""
Ask Qwen3.6-35B to implement the Mission Control crate based on its own spec.
Then build and deploy it.
"""
import json
import urllib.request
import os

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"
SPEC_PATH = "/home/cesarops/wreckhunter2000-1/docs/mission_control_spec.md"
CRATE_DIR = "/home/cesarops/wreckhunter2000-1/cesarops-mission-control"

# Load the spec
with open(SPEC_PATH) as f:
    spec = f.read()
print(f"Loaded spec: {len(spec)} chars")

# Get patterns from nautivecs
print("=== Querying nautivecs ===")
queries = [
    "axum router serve static files websocket handler",
    "thought engine process task dispatch clients",
]
all_context = []
for q in queries:
    payload = json.dumps({"query": q, "top_k": 2, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        if resp.get("context_block"):
            all_context.append(resp["context_block"])
        print(f"  [{q[:50]}] -> {len(resp['results'])} results")
    except Exception as e:
        print(f"  [{q[:50]}] -> ERROR: {e}")

combined_context = "\n\n".join(all_context)

print("\n=== Generating Mission Control implementation ===")

system_msg = f"""You are CESAROPS (Qwen3.6-35B). Implement the Mission Control crate from your spec.

RUST CODEGEN RULES:
- Use enum dispatch, NOT trait objects for async
- Compute derived values BEFORE moving into structs
- Use Vec<(&str, &str)> for HTTP params
- Annotate .collect() types when inference fails
- Match arms must return consistent types

Spec (you wrote this):
{spec[:8000]}

Codebase patterns:
{combined_context[:4000]}"""

user_msg = """Implement cesarops-mission-control as a Rust crate. Keep it MINIMAL but WORKING.

Requirements:
1. axum server on port 3000
2. Embed the HTML/CSS/JS from the spec as a static string (include_str! or inline)
3. POST /task endpoint that routes to KoboldCPP based on mode
4. GET /status endpoint checking service health
5. GET /presets endpoint returning hardcoded presets for now
6. WebSocket /ws for streaming progress (basic implementation)

The HTML should have the 3 big buttons (CODE/SCAN/RESEARCH), voice input, and progress display exactly as in the spec.

For the backend logic, keep it simple for MVP:
- All modes call KoboldCPP with a mode-specific system prompt
- Nautivecs is queried for context before sending to KoboldCPP
- Results streamed back via WebSocket

Output format:
=== FILE: Cargo.toml ===
[content]

=== FILE: src/main.rs ===
[content - the ENTIRE server in one file for simplicity. Include the HTML as a const str.]

Keep it under 400 lines. This must compile and run TODAY."""

request_body = {
    "model": "koboldcpp/Qwen3.6-35B-A3B-MXFP4_MOE",
    "messages": [
        {"role": "system", "content": system_msg},
        {"role": "user", "content": user_msg}
    ],
    "max_tokens": 16384,
    "temperature": 0.3,
    "top_p": 0.9,
    "presence_penalty": 0.8
}

print(f"System: {len(system_msg)} chars | User: {len(user_msg)} chars")
print("Generating...")

payload = json.dumps(request_body).encode()
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")

try:
    resp = json.loads(urllib.request.urlopen(req, timeout=1800).read())
    content = resp["choices"][0]["message"]["content"]

    with open("/home/cesarops/wreckhunter2000-1/docs/mission_control_impl.md", "w") as f:
        f.write(content)

    print(f"\nGenerated: {len(content)} chars")

    # Parse files
    import re
    file_pattern = r'=== FILE: (.+?) ==='
    parts = re.split(file_pattern, content)

    os.makedirs(f"{CRATE_DIR}/src", exist_ok=True)
    files_written = 0
    if len(parts) > 1:
        for i in range(1, len(parts), 2):
            filename = parts[i].strip()
            file_content = parts[i+1].strip() if i+1 < len(parts) else ""
            file_content = re.sub(r'^```\w*\n?', '', file_content)
            file_content = re.sub(r'\n?```\s*$', '', file_content)
            file_content = file_content.strip()

            filepath = f"{CRATE_DIR}/{filename}"
            os.makedirs(os.path.dirname(filepath), exist_ok=True)
            with open(filepath, "w") as f:
                f.write(file_content + "\n")
            print(f"  Written: {filename} ({len(file_content)} bytes)")
            files_written += 1

    print(f"\nFiles: {files_written}")

    if files_written > 0:
        # Add to workspace
        import pathlib
        ws_path = pathlib.Path("/home/cesarops/wreckhunter2000-1/Cargo.toml")
        ws = ws_path.read_text()
        if "cesarops-mission-control" not in ws:
            ws = ws.replace(
                '"cesarops-thought-engine"',
                '"cesarops-thought-engine", "cesarops-mission-control"'
            )
            ws_path.write_text(ws)
            print("Added to workspace")

        # Build
        import subprocess
        print("\n=== Building ===")
        result = subprocess.run(
            ["bash", "-c", "source ~/.cargo/env && cd /home/cesarops/wreckhunter2000-1 && cargo build --release -p cesarops-mission-control 2>&1"],
            capture_output=True, text=True, timeout=300
        )
        output = result.stdout + result.stderr
        if "Finished" in output:
            print("BUILD SUCCESS")
            # Deploy as service
            service = """[Unit]
Description=CESAROPS Mission Control Web UI
After=network.target koboldcpp.service nautivecs-server.service
Wants=koboldcpp.service nautivecs-server.service

[Service]
Type=simple
User=cesarops
WorkingDirectory=/home/cesarops/wreckhunter2000-1
ExecStart=/home/cesarops/wreckhunter2000-1/target/release/cesarops-mission-control
Environment="KOBOLD_URL=http://localhost:5001/v1"
Environment="NAUTIVECS_URL=http://localhost:5003"
Environment="PORT=3000"
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
SyslogIdentifier=mission-control

[Install]
WantedBy=multi-user.target
"""
            with open("/etc/systemd/system/mission-control.service", "w") as f:
                f.write(service)
            subprocess.run(["bash", "-c", "echo cesarops | sudo -S systemctl daemon-reload && echo cesarops | sudo -S systemctl enable mission-control && echo cesarops | sudo -S systemctl start mission-control"], capture_output=True, text=True)
            import time
            time.sleep(3)
            # Check health
            try:
                health = urllib.request.urlopen("http://localhost:3000/status", timeout=5).read()
                print(f"DEPLOYED AND RUNNING: {health.decode()}")
            except:
                print("Service started but not responding yet on :3000")
                print("Check: journalctl -u mission-control -n 20")
        else:
            # Print errors
            errors = [l for l in output.split('\n') if 'error' in l.lower()]
            print(f"BUILD FAILED ({len(errors)} errors)")
            for e in errors[:10]:
                print(f"  {e}")

except Exception as e:
    print(f"ERROR: {e}")
    import traceback
    traceback.print_exc()
