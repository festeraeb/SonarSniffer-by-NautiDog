"""
Transfer local TIF data to i7 via SFTP, then kick off a scan on i7.
Runs asynchronously — call this script and it will report progress.
"""
import paramiko
import sys
from pathlib import Path
import os

HOST, USER, PASS = "10.0.0.56", "cesarops", "cesarops"
LOCAL_DOWNLOADS = Path(__file__).parent.parent / "downloads"
REMOTE_BASE = "/home/cesarops/downloads"

def run(ssh, cmd, timeout=60):
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="replace").strip()
    err = e.read().decode(errors="replace").strip()
    return out or err

def upload_lake(sftp, lake):
    local_dir = LOCAL_DOWNLOADS / lake
    if not local_dir.exists():
        return 0
    tifs = list(local_dir.rglob("*.tif"))
    if not tifs:
        return 0
    
    # Ensure remote dir exists
    remote_dir = f"{REMOTE_BASE}/{lake}"
    print(f"  Uploading {len(tifs)} files from {lake} ...", end="", flush=True)
    
    for i, tif in enumerate(tifs):
        rel = tif.relative_to(local_dir)
        remote_path = f"{remote_dir}/{str(rel).replace(chr(92), '/')}"
        # Ensure parent dir exists
        parent = "/".join(remote_path.split("/")[:-1])
        try:
            sftp.stat(parent)
        except FileNotFoundError:
            # mkdir -p via SSH
            pass
        try:
            sftp.put(str(tif), remote_path)
        except Exception as ex:
            # mkdir the parent and retry
            run_ssh.exec_command(f"mkdir -p '{parent}'")
            try:
                sftp.put(str(tif), remote_path)
            except Exception:
                print(f"    SKIP {tif.name}: {ex}")
        if (i+1) % 10 == 0:
            print(f" {i+1}/{len(tifs)}", end="", flush=True)
    print(f" done ({len(tifs)} files)")
    return len(tifs)

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
print(f"Connecting to {HOST} ...")
c.connect(HOST, username=USER, password=PASS, timeout=10)
run_ssh = c  # alias for nested func

# Create remote dirs
for lake in ["erie", "huron", "michigan", "superior", "straits", "ontario"]:
    run(c, f"mkdir -p {REMOTE_BASE}/{lake}")

# Upload via SFTP
sftp = c.open_sftp()
total = 0
for lake in ["michigan", "superior", "huron", "erie"]:
    total += upload_lake(sftp, lake)
sftp.close()
print(f"\n[+] Total uploaded: {total} TIF files")

# Verify count on remote
count = run(c, f"find {REMOTE_BASE} -name '*.tif' | wc -l")
print(f"[+] Remote TIF count: {count}")

# Start scan on i7 (background nohup process)
print("\n[+] Starting scan on i7 ...")
scan_cmd = (
    f"cd ~/wreckhunter2000-1 && "
    f"CESAROPS_DATA_DIR={REMOTE_BASE} "
    f"nohup .venv/bin/python lake_michigan_scan.py "
    f"> ~/scan_log_$(date +%Y%m%d_%H%M%S).txt 2>&1 &"
)
run(c, scan_cmd, timeout=10)

# Check it started
import time; time.sleep(2)
ps_out = run(c, "pgrep -la python | grep lake_michigan || echo 'not found'")
print(f"[+] Scan process: {ps_out}")
tail = run(c, "tail -5 ~/scan_log_*.txt 2>/dev/null | head -20 || echo 'log not written yet'")
print(f"[+] Scan log tail:\n{tail}")

c.close()
print("\n[DONE] i7 scan started. Check progress:")
print("  python scripts/_i7_scan_status.py")
