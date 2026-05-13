#!/usr/bin/env python3
"""
Install / manage wrecks_api as a systemd service on the Pi (conductor node).

Architecture:
  Pi (cesarops-node) = conductor + API host (home.cesarops.com:8099)
  i7 / other nodes   = workers, connect back to Pi

Connects via SSH to Pi and:
  1. Writes /etc/systemd/system/wrecks-api.service
  2. Enables and starts it (or restarts if already installed)

Usage:
    python scripts/install_api_service.py            # install + start
    python scripts/install_api_service.py --restart  # restart service
    python scripts/install_api_service.py --status   # show service status
    python scripts/install_api_service.py --sync-db  # re-upload wrecks.db from local repo
"""

import argparse
import os
import sys
from pathlib import Path

import paramiko
from dotenv import load_dotenv

REPO = Path(__file__).resolve().parents[1]
load_dotenv(REPO / ".env")

I7_HOST = os.environ.get("I7_HOST", "100.85.138.4")
I7_USER = os.environ.get("I7_USER", "cesarops")
I7_PASS = os.environ.get("I7_PASS", "")
I7_KEY  = os.environ.get("I7_KEY", "")

PI_HOST = os.environ.get("PI_TAILSCALE", "100.127.66.32")
PI_USER = os.environ.get("PI_USER", "pi")
PI_PASS = os.environ.get("PI_PASS", "admin")

API_PORT = 8099  # wrecks_api port

SERVICE_NAME = "wrecks-api"

SERVICE_TEMPLATE = """\
[Unit]
Description=CESAROPS Wrecks API (FastAPI/uvicorn)
After=network.target

[Service]
Type=simple
User={user}
WorkingDirectory={repo}
ExecStart={python} -m uvicorn wrecks_api.app:app --host 0.0.0.0 --port {port} --workers 2
Restart=always
RestartSec=5
Environment="DB_PATH={repo}/db/wrecks.db"
Environment="API_BASE_URL=http://home.cesarops.com:{port}"

[Install]
WantedBy=multi-user.target
"""


def _i7_connect_kwargs() -> dict:
    kw = dict(username=I7_USER, timeout=20)
    key_path = os.path.expanduser(I7_KEY) if I7_KEY else ""
    if key_path and os.path.exists(key_path):
        kw["key_filename"] = key_path
    if I7_PASS:
        kw["password"] = I7_PASS
    return kw


def ssh_connect() -> paramiko.SSHClient:
    """Try direct SSH first; fall back to Pi jump host."""
    # --- Direct ---
    try:
        c = paramiko.SSHClient()
        c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        c.connect(I7_HOST, **_i7_connect_kwargs())
        print(f"[SSH] Direct connection to {I7_HOST} OK")
        return c
    except Exception as direct_err:
        print(f"[SSH] Direct failed ({direct_err}), trying Pi jump host …")

    # --- Jump via Pi ---
    try:
        jump = paramiko.SSHClient()
        jump.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        jump.connect(PI_HOST, username=PI_USER, password=PI_PASS, timeout=15)
        transport = jump.get_transport()
        sock = transport.open_channel("direct-tcpip", (I7_HOST, 22), ("127.0.0.1", 0))
        c = paramiko.SSHClient()
        c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        c.connect(I7_HOST, sock=sock, **_i7_connect_kwargs())
        print(f"[SSH] Connected to {I7_HOST} via Pi jump OK")
        return c
    except Exception as jump_err:
        raise RuntimeError(
            f"Both SSH paths failed.\n  Direct: {direct_err}\n  Jump:   {jump_err}\n"
            f"  Is the i7 on and reachable?"
        ) from jump_err


def run(c: paramiko.SSHClient, cmd: str, check: bool = True) -> str:
    _, out, err = c.exec_command(cmd)
    stdout = out.read().decode().strip()
    stderr = err.read().decode().strip()
    if check and out.channel.recv_exit_status() != 0 and stderr:
        print(f"[WARN] {stderr[:200]}")
    return stdout


def install(c: paramiko.SSHClient):
    repo = "/home/pi/wreckhunter2000-1"
    python = "/usr/bin/python3"
    uvicorn = "/home/pi/.local/bin/uvicorn"

    # Install uvicorn if needed
    has_uvicorn = run(c, f"{uvicorn} --version 2>/dev/null", check=False)
    if not has_uvicorn:
        print("[INFO] Installing uvicorn + fastapi …")
        run(c, f"{python} -m pip install uvicorn[standard] fastapi --break-system-packages --quiet")

    # Write service file via tmp
    import io
    service_content = SERVICE_TEMPLATE.format(
        user="pi", repo=repo, python=python, uvicorn=uvicorn, port=API_PORT
    )
    sftp = c.open_sftp()
    sftp.putfo(io.BytesIO(service_content.encode()), "/tmp/wrecks-api.service")
    sftp.close()

    run(c, f"sudo mv /tmp/wrecks-api.service /etc/systemd/system/{SERVICE_NAME}.service")
    print("[INFO] Service file written.")

    run(c, "sudo systemctl daemon-reload")
    run(c, f"sudo systemctl enable {SERVICE_NAME}")
    run(c, f"sudo systemctl restart {SERVICE_NAME}")
    print("[INFO] Service enabled and started.")

    import time; time.sleep(2)
    status = run(c, f"sudo systemctl status {SERVICE_NAME} --no-pager -l", check=False)
    print(status)
    print(f"\n[DONE] API → http://{PI_HOST}:{API_PORT}")
    print(f"       External: http://home.cesarops.com:{API_PORT}  (needs router port-forward → {PI_HOST})")


def sync_db(c: paramiko.SSHClient):
    """Upload local wrecks.db to Pi."""
    local_db = REPO / "db" / "wrecks.db"
    if not local_db.exists():
        sys.exit("[ERROR] db/wrecks.db not found locally.")
    size_mb = local_db.stat().st_size / 1024 / 1024
    print(f"[SYNC] Uploading {size_mb:.1f} MB wrecks.db to Pi …")
    sftp = c.open_sftp()
    sftp.put(str(local_db), "/home/pi/wreckhunter2000-1/db/wrecks.db")
    sftp.close()
    print("[SYNC] Done. Restarting API …")
    run(c, f"sudo systemctl restart {SERVICE_NAME}")
    import time; time.sleep(2)
    print(run(c, "curl -s http://localhost:8099/stats", check=False))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--restart",  action="store_true")
    parser.add_argument("--status",   action="store_true")
    parser.add_argument("--sync-db",  action="store_true", help="Re-upload wrecks.db to Pi")
    args = parser.parse_args()

    print(f"[SSH] Connecting to Pi ({PI_USER}@{PI_HOST}) …")
    try:
        c = _pi_connect()
    except Exception as e:
        sys.exit(f"[ERROR] SSH to Pi failed: {e}")

    if args.status:
        print(run(c, f"sudo systemctl status {SERVICE_NAME} --no-pager -l", check=False))
    elif args.restart:
        run(c, f"sudo systemctl restart {SERVICE_NAME}")
        print(run(c, f"sudo systemctl status {SERVICE_NAME} --no-pager", check=False))
    elif args.sync_db:
        sync_db(c)
    else:
        install(c)

    c.close()


def _pi_connect() -> paramiko.SSHClient:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(PI_HOST, username=PI_USER, password=PI_PASS, timeout=15)
    return c


if __name__ == "__main__":
    main()
