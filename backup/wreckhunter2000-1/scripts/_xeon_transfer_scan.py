"""
Transfer local TIF data to Xeon (cesarops2, 10.0.0.129) via SFTP, then kick off a scan.
Runs synchronously and reports progress.
"""
import paramiko
import sys
from pathlib import Path
import os
import time

HOST, USER, PASS = "10.0.0.129", "cesarops1", "cesarops1"
LOCAL_DOWNLOADS = Path(__file__).parent.parent / "downloads"
REMOTE_BASE = "/home/cesarops1/downloads"
REPO_DIR = "/home/cesarops1/wreckhunter2000-1"

def run(ssh, cmd, timeout=60):
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="replace").strip()
    err = e.read().decode(errors="replace").strip()
    return out or err

def upload_lake(sftp, ssh, lake):
    local_dir = LOCAL_DOWNLOADS / lake
    if not local_dir.exists():
        return 0
    tifs = list(local_dir.rglob("*.tif"))
    if not tifs:
        return 0

    remote_dir = f"{REMOTE_BASE}/{lake}"
    print(f"  Uploading {len(tifs)} files from {lake} ...", end="", flush=True)

    for i, tif in enumerate(tifs):
        rel = tif.relative_to(local_dir)
        remote_path = f"{remote_dir}/{str(rel).replace(chr(92), '/')}"
        parent = "/".join(remote_path.split("/")[:-1])
        try:
            sftp.stat(parent)
        except FileNotFoundError:
            run(ssh, f"mkdir -p '{parent}'")
        try:
            sftp.put(str(tif), remote_path)
        except Exception as ex:
            run(ssh, f"mkdir -p '{parent}'")
            try:
                sftp.put(str(tif), remote_path)
            except Exception:
                print(f"    SKIP {tif.name}: {ex}")
        if (i + 1) % 10 == 0:
            print(f" {i + 1}/{len(tifs)}", end="", flush=True)
    print(f" done ({len(tifs)} files)")
    return len(tifs)

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
print(f"Connecting to {HOST} (cesarops2) ...")
c.connect(HOST, username=USER, password=PASS, timeout=10)
print(f"[+] Connected — hostname: {run(c, 'hostname')}")

# Create remote dirs
for lake in ["erie", "huron", "michigan", "superior", "straits", "ontario"]:
    run(c, f"mkdir -p {REMOTE_BASE}/{lake}")
run(c, f"mkdir -p {REPO_DIR}/outputs/probes {REPO_DIR}/outputs")

# Upload via SFTP
sftp = c.open_sftp()
total = 0
for lake in ["michigan", "superior", "huron", "erie"]:
    total += upload_lake(sftp, c, lake)
sftp.close()
print(f"\n[+] Total uploaded: {total} TIF files")

# Verify count on remote
count = run(c, f"find {REMOTE_BASE} -name '*.tif' | wc -l")
print(f"[+] Remote TIF count: {count}")

if total == 0 and int(count) == 0:
    print("[!] No TIF files found locally or remotely — cannot start scan")
    c.close()
    sys.exit(0)

# Start scan on Xeon (background nohup process)
print("\n[+] Starting lake_michigan_scan.py on Xeon ...")
log_name = f"scan_log_xeon_$(date +%Y%m%d_%H%M%S).txt"
scan_cmd = (
    f"cd {REPO_DIR} && "
    f"CESAROPS_DATA_DIR={REMOTE_BASE} "
    f"nohup .venv/bin/python lake_michigan_scan.py "
    f"> ~/{log_name} 2>&1 &"
)
run(c, scan_cmd, timeout=10)

time.sleep(2)
ps_out = run(c, "pgrep -la python | grep lake_michigan || echo 'not found'")
print(f"[+] Scan process: {ps_out}")
tail = run(c, f"tail -5 ~/scan_log_xeon_*.txt 2>/dev/null | head -20 || echo 'log not written yet'")
print(f"[+] Scan log tail:\n{tail}")

c.close()
print("\n[DONE] Xeon scan started.")
print("  Check status:  python scripts/_xeon_scan_status.py")
