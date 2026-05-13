"""Install all scan deps on the i7 node."""
import paramiko

HOST, USER, PASS = "10.0.0.56", "cesarops", "cesarops"

def run(ssh, cmd, timeout=300, label=None):
    if label:
        print(f"  [{label}] ...", end=" ", flush=True)
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="replace").strip()
    err = e.read().decode(errors="replace").strip()
    result = out or err or "(ok)"
    print(result[-200:])
    return result

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASS, timeout=10)
print(f"[+] Connected to {HOST}")

# Find gdal version
gdal_ver = run(c, "gdal-config --version 2>/dev/null || dpkg-query -W -f='${Version}' libgdal-dev 2>/dev/null | cut -d'-' -f1", label="gdal version")

# Install all deps at once  
print("\n[1] Installing Python deps in venv ...")
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/pip install --quiet "
    "numpy scipy matplotlib pillow requests tqdm paramiko "
    "scikit-learn xgboost lightgbm shapely pyproj "
    "rasterio geopandas fiona "
    "fastapi uvicorn pydantic "
    "2>&1 | tail -8",
    timeout=600, label="pip install core")

# Rasterio may need extra help
run(c, "cd ~/wreckhunter2000-1 && .venv/bin/pip install --quiet rasterio 2>&1 | tail -3",
    timeout=120, label="rasterio")

# Verify
print("\n[2] Verifying imports ...")
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/python -c 'import numpy, scipy, rasterio, sklearn, xgboost, lightgbm; "
    "print(f\"numpy {numpy.__version__}, rasterio {rasterio.__version__}, lgbm {lightgbm.__version__}\")'",
    label="imports")

# CUDA
run(c,
    "cd ~/wreckhunter2000-1 && "
    ".venv/bin/python -c 'import torch; "
    "print(f\"torch {torch.__version__}, CUDA {torch.cuda.is_available()}, GPU: {torch.cuda.get_device_name(0) if torch.cuda.is_available() else None}\")'",
    label="torch/cuda")

# Create downloads symlink from somewhere sensible
run(c, "mkdir -p ~/downloads/erie ~/downloads/huron ~/downloads/michigan ~/downloads/superior ~/downloads/jobs",
    label="create download dirs")

print("\n[DONE] i7 deps installed.")
print("Next: run scripts/_i7_start_download.py to grab HLS tiles, OR rsync laptop data:")
print("  rsync -avz --progress downloads/ cesarops@10.0.0.56:~/wreckhunter2000-1/downloads/")
