"""Check i7 state and install GDAL, then start a scan."""
import paramiko, sys

HOST, USER, PASS = "10.0.0.56", "cesarops", "cesarops"

def run(ssh, cmd, timeout=60):
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="replace").strip()
    err = e.read().decode(errors="replace").strip()
    print(f"  {out or err or '(ok)'}")
    return out

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASS, timeout=10)
print(f"[+] Connected to {HOST}")

# Fix GDAL
print("\n[1] Installing GDAL system library ...")
run(c, "sudo DEBIAN_FRONTEND=noninteractive apt-get install -y libgdal-dev gdal-bin 2>&1 | tail -3", timeout=120)

# Re-install rasterio + gdal pip packages against system gdal
print("\n[2] Re-installing rasterio/GDAL pip packages ...")
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/pip install --quiet rasterio 'GDAL==$(gdal-config --version)' 2>&1 | tail -5",
    timeout=120)

# Check what's importable
print("\n[3] Checking scan imports ...")
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/python -c 'import numpy, scipy, rasterio, requests; print(\"core imports OK\")'")

# Check CUDA
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/python -c 'import torch; print(f\"CUDA: {torch.cuda.is_available()}, GPU: {torch.cuda.get_device_name(0) if torch.cuda.is_available() else None}\")'")

# Check for any data on i7
print("\n[4] Data on i7 ...")
run(c, "find ~/downloads -name '*.tif' 2>/dev/null | wc -l || echo 'no downloads dir'")
run(c, "ls ~/downloads 2>/dev/null || echo 'no downloads dir'")

# Check lake_michigan_scan imports (the processing engine)
print("\n[5] Test scan engine syntax ...")
run(c, "cd ~/wreckhunter2000-1 && .venv/bin/python -m py_compile lake_michigan_scan.py && echo 'syntax OK'")
run(c, "cd ~/wreckhunter2000-1 && .venv/bin/python -m py_compile lake_erie_scan.py && echo 'syntax OK'")

# Check if scan can run in --help / dry-run mode
print("\n[6] Scan --help ...")
run(c, "cd ~/wreckhunter2000-1 && .venv/bin/python lake_michigan_scan.py --help 2>&1 | head -10")

c.close()
print("\n[DONE] i7 state check complete.")
