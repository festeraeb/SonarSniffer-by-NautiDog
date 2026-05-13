#!/usr/bin/env python3
"""Status check: laptop drive, Xeon scan, i7 scan/TPU."""
import sys, os, subprocess, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent.parent))

import paramiko

I7   = ("10.0.0.56",  "cesarops",  "cesarops")
XEON = ("10.0.0.129", "cesarops1", "cesarops1")

def ssh_run(host, user, pw, cmd, timeout=20):
    try:
        c = paramiko.SSHClient()
        c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        c.connect(host, username=user, password=pw, timeout=8)
        _, o, e = c.exec_command(cmd, timeout=timeout)
        out = (o.read() + e.read()).decode(errors="replace").strip()
        c.close()
        return out
    except Exception as ex:
        return f"ERROR: {ex}"

# ── Laptop ────────────────────────────────────────────────────────────────
print("=" * 60)
print("LAPTOP")
print("=" * 60)

from drive_discovery import find_armor_drive
drive = find_armor_drive(quiet=True)
print(f"  drive_discovery:  {drive}")

if drive:
    db    = drive / "LAKE_MICHIGAN_CENSUS_2026.db"
    dls   = drive / "downloads"
    tifs  = list(dls.rglob("*.tif")) if dls.exists() else []
    # dedup because Windows case-insensitive FS can double-count
    tif_set = {str(t).lower() for t in tifs}
    print(f"  DB exists:        {db.exists()} ({db.stat().st_size//1024//1024} MB)" if db.exists() else f"  DB exists:        False")
    print(f"  TIF count:        {len(tif_set)}")
else:
    print("  DRIVE NOT FOUND — this is why laptop can't process files")

# Check if lake_michigan_scan is running on laptop (via tasklist)
try:
    tasks = subprocess.check_output(
        ["tasklist", "/fi", "imagename eq python.exe", "/fo", "csv"],
        text=True, stderr=subprocess.DEVNULL
    )
    scan_running = "lake_michigan_scan" in tasks.lower()
    # Better: check by command line
    wmic = subprocess.check_output(
        ['wmic', 'process', 'where', "name='python.exe'", 'get', 'commandline', '/format:list'],
        text=True, stderr=subprocess.DEVNULL
    )
    py_procs = [l for l in wmic.splitlines() if l.strip().startswith("CommandLine=")]
    print(f"  Python processes: {len(py_procs)}")
    for p in py_procs[:5]:
        print(f"    {p.strip()[:100]}")
except Exception as ex:
    print(f"  process check: {ex}")

# ── i7 ─────────────────────────────────────────────────────────────────────
print()
print("=" * 60)
print("i7  (10.0.0.56)")
print("=" * 60)
i7_status = ssh_run(*I7, """
    echo "--- mounts ---"
    mount | grep -E 'cesarops|armor|downloads' | head -5
    echo "--- scan processes ---"
    pgrep -la python | grep -E 'lake_michigan|scan|cpu_pass' || echo none
    echo "--- tpu server ---"
    pgrep -la python | grep tpu || echo tpu_not_running
    echo "--- scan log tail ---"
    tail -6 ~/scan_log_xeon_now.txt 2>/dev/null || tail -6 ~/scan_log_i7.txt 2>/dev/null || echo no_log
    echo "--- cpu pass log ---"
    tail -4 ~/cpu_passes_log.txt 2>/dev/null || echo no_cpu_log
    echo "--- armor du ---"
    du -sh /mnt/cesarops-armor/downloads/* 2>/dev/null | head -6
""", timeout=30)
print(i7_status)

# ── Xeon ───────────────────────────────────────────────────────────────────
print()
print("=" * 60)
print("Xeon  (10.0.0.129)")
print("=" * 60)
xeon_status = ssh_run(*XEON, """
    echo "--- scan processes ---"
    pgrep -la python | grep -E 'lake_michigan|scan' || echo none
    echo "--- sshfs mounts ---"
    mount | grep -E 'sshfs|i7|downloads|armor' | head -5 || echo no_sshfs
    echo "--- tif count ---"
    find ~/wreckhunter2000-1/downloads -name '*.tif' -follow 2>/dev/null | wc -l
    echo "--- scan log tail ---"
    tail -8 ~/scan_log_xeon_now.txt 2>/dev/null || echo no_log
    echo "--- df ---"
    df -h / | tail -1
""", timeout=30)
print(xeon_status)
