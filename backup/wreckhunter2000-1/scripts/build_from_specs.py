#!/usr/bin/env python3
"""
Read the specs the 35B produced, have it write the actual crate code, then compile.
Runs autonomously on T440 — no human needed.
"""
import json
import urllib.request
import subprocess
import time
from pathlib import Path

KOBOLD = "http://localhost:5001/v1"
SPECS_DIR = Path("/mnt/data-external/cesarops/analysis-v2")
REPO = Path("/home/cesarops/wreckhunter2000-1")

def call_35b(system, user, max_tokens=16384):
    payload = json.dumps({
        "model": "auto",
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "max_tokens": max_tokens, "temperature": 0.3, "stream": False
    }).encode()
    req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    resp = json.loads(urllib.request.urlopen(req, timeout=1200).read())
    return resp["choices"][0]["message"]["content"]

def parse_and_write_files(content, base_dir):
    """Parse === FILE: path === markers and write files."""
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

def try_build(crate_name):
    """Try to build a crate, return success and errors."""
    result = subprocess.run(
        ["bash", "-c", f"source ~/.cargo/env && cd {REPO} && cargo build --release -p {crate_name} 2>&1"],
        capture_output=True, text=True, timeout=300
    )
    output = result.stdout + result.stderr
    success = "Finished" in output
    errors = [l for l in output.split('\n') if 'error[' in l]
    return success, errors, output

# === BUILD ADAPTIVE PIPELINE ===
print("=" * 60)
print("BUILDING: cesarops-adaptive")
print("=" * 60)

spec = (SPECS_DIR / "adaptive_pipeline.md").read_text()
print(f"Spec loaded: {len(spec)} chars")

# Check if files already exist
adaptive_dir = REPO / "cesarops-adaptive"
if not (adaptive_dir / "Cargo.toml").exists():
    print("Asking 35B to write implementation from spec...")
    impl = call_35b(
        "You are CESAROPS. Write COMPLETE compilable Rust code from this spec. Use enum dispatch not trait objects. Compute values before moving. Output files with === FILE: path === markers.",
        f"Implement this spec as a Rust crate. Output ALL files:\n\n{spec[:12000]}"
    )
    written = parse_and_write_files(impl, REPO)
    print(f"Files written: {written}")
else:
    print("Crate already exists, trying build...")

# Add to workspace if needed
ws = (REPO / "Cargo.toml").read_text()
if "cesarops-adaptive" not in ws:
    ws = ws.replace('"cesarops-detection"', '"cesarops-detection", "cesarops-adaptive"')
    (REPO / "Cargo.toml").write_text(ws)
    print("Added to workspace")

# Try build
success, errors, output = try_build("cesarops-adaptive")
if success:
    print("BUILD SUCCESS")
else:
    print(f"BUILD FAILED ({len(errors)} errors)")
    # Ask 35B to fix
    if errors:
        print("Asking 35B to fix errors...")
        # Read the source files
        src_files = list((adaptive_dir / "src").glob("*.rs")) if (adaptive_dir / "src").exists() else []
        src_content = "\n\n".join([f"// {f.name}\n{f.read_text()}" for f in src_files[:5]])
        fix = call_35b(
            "Fix these Rust compiler errors. Output the COMPLETE fixed files with === FILE: path === markers.",
            f"Errors:\n{chr(10).join(errors[:10])}\n\nSource:\n{src_content[:8000]}"
        )
        parse_and_write_files(fix, REPO)
        # Retry
        success2, errors2, _ = try_build("cesarops-adaptive")
        if success2:
            print("BUILD SUCCESS (after fix)")
        else:
            print(f"Still failing ({len(errors2)} errors). Manual review needed.")

print("\n" + "=" * 60)
print("DONE")
print("=" * 60)
