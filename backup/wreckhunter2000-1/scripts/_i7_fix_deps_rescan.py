"""Install missing scan deps on i7 and relaunch the scan."""
import paramiko, time

HOST, USER, PASS = "10.0.0.56", "cesarops", "cesarops"
REMOTE_DATA = "/home/cesarops/downloads"

def run(ssh, cmd, timeout=120):
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="replace").strip()
    err = e.read().decode(errors="replace").strip()
    result = out or err or "(ok)"
    print(f"  {result[-400:]}")
    return result

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASS, timeout=10)
print(f"[+] Connected")

# Check what's missing by running python -c imports
print("\n[1] Finding missing imports ...")
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/python -c 'import simplekml, pyproj, shapely, fiona, geopandas, cupy' 2>&1")

# Install everything that could be missing
print("\n[2] Installing missing deps ...")
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/pip install --quiet simplekml pyproj shapely 2>&1 | tail -5",
    timeout=120)

# Verify all scan imports
print("\n[3] Verify all scan imports ...")
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/python -c '"
    "import numpy, scipy, rasterio, requests, simplekml, shapely; "
    "print(\"All core imports OK\")"
    "'")

# Kill any stale scan processes
run(c, "pkill -f lake_michigan_scan || true", timeout=5)
time.sleep(1)

# Relaunch scan
print("\n[4] Launching scan on i7 ...")
scan_cmd = (
    "cd ~/wreckhunter2000-1 && "
    f"CESAROPS_DATA_DIR={REMOTE_DATA} "
    "nohup .venv/bin/python lake_michigan_scan.py "
    "> ~/scan_$(date +%Y%m%d_%H%M%S).log 2>&1 &"
)
_, o, e = c.exec_command(scan_cmd, timeout=10)
time.sleep(3)

# Confirm running
ps = run(c, "pgrep -la python | grep -i lake || echo 'not started'", timeout=5)
log = run(c, "tail -15 ~/scan_*.log 2>/dev/null | head -20 || echo 'no log yet'", timeout=5)
print(f"\n[+] Scan log:\n{log}")

c.close()
print("\n[DONE] i7 scan relaunched.")
