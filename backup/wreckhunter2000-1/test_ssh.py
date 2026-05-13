#!/usr/bin/env python3
"""SSH connectivity test — key-based auth with password fallback from .env."""
import paramiko
import os
import sys
from pathlib import Path

def _load_env(path: Path) -> dict:
    env = {}
    if path.exists():
        for line in path.read_text(encoding='utf-8').splitlines():
            line = line.strip()
            if line and not line.startswith('#') and '=' in line:
                k, _, v = line.partition('=')
                env[k.strip()] = v.strip()
    return env

_dotenv = _load_env(Path(__file__).parent / ".env")

HOST = os.environ.get("I7_HOST", _dotenv.get("I7_HOST", "10.0.0.56"))
USER = os.environ.get("I7_USER", _dotenv.get("I7_USER", "cesarops"))
PASSWORD = os.environ.get("I7_PASS", _dotenv.get("I7_PASS", ""))

client = paramiko.SSHClient()
client.load_system_host_keys()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())

print(f"Connecting to {USER}@{HOST}:22 (key auth → password fallback)...", flush=True)
try:
    client.connect(
        HOST,
        port=22,
        username=USER,
        timeout=10,
        allow_agent=True,
        look_for_keys=True,
    )
    print("✓ SSH key auth succeeded!", flush=True)
except Exception as e:
    if PASSWORD:
        print(f"  Key auth failed ({e}), trying password from .env...", flush=True)
        try:
            client.connect(
                HOST,
                port=22,
                username=USER,
                password=PASSWORD,
                timeout=10,
                allow_agent=False,
                look_for_keys=False,
            )
            print("✓ Password auth succeeded!", flush=True)
        except Exception as e2:
            print(f"✗ Password auth also failed: {e2}", flush=True)
            sys.exit(1)
    else:
        print(f"✗ Key auth failed and no password in .env: {e}", flush=True)
        sys.exit(1)

try:
    _, out, _ = client.exec_command('echo SUCCESS && hostname && uname -a', timeout=5)
    print(out.read().decode().strip(), flush=True)

    _, out, _ = client.exec_command('nvidia-smi -L 2>/dev/null || echo "No NVIDIA GPU"', timeout=5)
    print(f"GPU: {out.read().decode().strip()}", flush=True)

    _, out, _ = client.exec_command('ls /dev/apex_* /dev/accel0 2>/dev/null || echo "No Coral TPU"', timeout=5)
    print(f"TPU: {out.read().decode().strip()}", flush=True)
finally:
    client.close()
