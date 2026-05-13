#!/usr/bin/env python3
"""
After the initial supervisor builds, extend it to manage ALL infrastructure:
- cloudflared tunnel
- drive mounts (RAID, external, network shares)
- samba server
- tailscale
- VS Code Server
"""
import json
import urllib.request
import time
from pathlib import Path

KOBOLD = "http://localhost:5001/v1"
REPO = Path("/home/cesarops/wreckhunter2000-1")

# Wait for the first build to finish
print("Waiting for initial supervisor build to complete...")
while True:
    import subprocess
    r = subprocess.run(["pgrep", "-f", "build_supervisor"], capture_output=True)
    if r.returncode != 0:
        break
    time.sleep(10)
    print("  still building...")

print("Initial build done. Extending with infrastructure services...")

# Read what was generated
supervisor_dir = REPO / "cesarops-supervisor" / "src"
existing_files = {}
if supervisor_dir.exists():
    for f in supervisor_dir.glob("*.rs"):
        existing_files[f.name] = f.read_text()

config_file = REPO / "cesarops-supervisor" / "supervisor.toml"
existing_config = config_file.read_text() if config_file.exists() else ""

payload = json.dumps({
    "model": "auto",
    "messages": [
        {"role": "system", "content": f"""You are extending the cesarops-supervisor crate.
The initial version manages LLM services. Now add infrastructure management.

Existing config.rs structure:
{existing_files.get('config.rs', 'not yet generated')[:3000]}

Existing supervisor.toml:
{existing_config[:2000]}

RUST CODEGEN RULES:
- Enum dispatch not trait objects
- Compute values before moving
- No unwraps on fallible operations"""},
        {"role": "user", "content": """Extend the supervisor to manage ALL infrastructure. Add these to the config and process management:

INFRASTRUCTURE SERVICES (managed processes):
1. cloudflared — tunnel daemon, binary at /usr/bin/cloudflared, runs with --token
2. smbd — Samba file sharing, binary at /usr/sbin/smbd
3. code-insiders — VS Code Server, binary at /usr/local/bin/code-insiders serve-web

DRIVE MOUNTS (check and remount if missing):
4. /mnt/data-external — UUID=dec00b8b-a95a-4c02-ae40-f7e6ab1b21e9 (ext4, USB SSD)
5. /codebase — /dev/sdb1 (xfs, RAID partition 1)
6. /data — /dev/sdb2 (xfs, RAID partition 2)
7. /mnt/cesarops2 — CIFS //100.102.158.111/storage
8. /mnt/cesarops3-storage — CIFS //100.105.77.74/storage
9. /mnt/scratch — CIFS //100.105.77.74/scratch

NETWORK MONITORING:
10. Tailscale — check tailscale0 interface is up, restart tailscaled if not
11. Both NICs (eno1, eno2) — check they have IPs

STARTUP ORDER:
1. Mount drives first (everything else depends on /codebase for models)
2. Start networking (tailscale, cloudflared)
3. Start samba (depends on drives being mounted)
4. Start LLM services (depends on models on /codebase)
5. Start application services (depends on LLM)

Write ONLY the new/modified files needed:

=== FILE: cesarops-supervisor/src/mounts.rs ===
[Drive mount checking and remounting logic — uses std::process::Command to call mount]

=== FILE: cesarops-supervisor/src/network.rs ===
[Network interface checking, tailscale health, cloudflared management]

=== FILE: cesarops-supervisor/supervisor.toml ===
[COMPLETE config with ALL services including infrastructure]

Update the existing files minimally — just add the new service types to the enum and startup order."""}
    ],
    "max_tokens": 16384,
    "temperature": 0.3,
    "stream": False
}).encode()

print("Asking 35B to extend supervisor with infrastructure...")
start = time.time()
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")
resp = json.loads(urllib.request.urlopen(req, timeout=1200).read())
content = resp["choices"][0]["message"]["content"]
elapsed = time.time() - start

output = Path("/mnt/data-external/cesarops/analysis-v2/supervisor_extend.md")
output.write_text(f"# Supervisor Extension — Infrastructure\n\n{content}\n")
print(f"Generated: {len(content)} chars in {elapsed:.0f}s")

# Parse and write files
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
        filepath = REPO / filename
        filepath.parent.mkdir(parents=True, exist_ok=True)
        filepath.write_text(file_content + "\n")
        print(f"  Written: {filename} ({len(file_content)} bytes)")
        files_written += 1

print(f"\nFiles: {files_written}")
print("Now rebuild the supervisor with: cargo build --release -p cesarops-supervisor")
