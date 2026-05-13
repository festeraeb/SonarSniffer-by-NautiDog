"""
One-shot Xeon (cesarops2, 10.0.0.129) bootstrap:
  - Clone repo (no existing files)
  - Set up venv + all scan deps
  - Copy TIF data from laptop
  - Launch scan
  - Update deploy scripts with new IP
"""
import paramiko
import sys
from pathlib import Path

HOST = "10.0.0.129"
USER = "cesarops1"
PASS = "cesarops1"

def load_env(p):
    env = {}
    try:
        for line in Path(p).read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                env[k.strip()] = v.strip()
    except Exception:
        pass
    return env

local_env = load_env(Path(__file__).parent.parent / ".env")
PAT = local_env.get("GITHUB_PAT", "")
EARTHDATA = local_env.get("EARTHDATA_TOKEN", "")

if not PAT:
    print("[ERROR] GITHUB_PAT not found in .env")
    sys.exit(1)

REPO_URL = f"https://festeraeb:{PAT}@github.com/festeraeb/wreckhunter2000.git"
BRANCH = "wreckhuntertools"
REPO_DIR = "~/wreckhunter2000-1"

def run(ssh, cmd, timeout=120, label=None, quiet=False):
    if label:
        print(f"  [{label}] ...", end=" ", flush=True)
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="replace").strip()
    err = e.read().decode(errors="replace").strip()
    result = out or err or "(ok)"
    if not quiet:
        print(result[-300:])
    return result

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
print(f"Connecting to {HOST} ({USER}) ...")
c.connect(HOST, username=USER, password=PASS, timeout=10)
print(f"[+] Connected to {HOST} — hostname: {run(c, 'hostname', quiet=True)}")

# ── 1. GPU check ──────────────────────────────────────────────────────────────
print("\n[1/6] GPU check ...")
run(c, "nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader 2>/dev/null || echo 'no nvidia-smi'", label="gpu")

# ── 2. Clone repo ─────────────────────────────────────────────────────────────
print("\n[2/6] Setting up git repo ...")
has_git = run(c, f"ls {REPO_DIR}/.git 2>/dev/null && echo HAS_GIT || echo NO_GIT", quiet=True)

if "NO_GIT" in has_git:
    has_dir = run(c, f"ls {REPO_DIR} 2>/dev/null && echo HAS_DIR || echo NO_DIR", quiet=True)
    if "NO_DIR" in has_dir:
        print("  No repo dir — cloning fresh ...")
        run(c, f"git clone --branch {BRANCH} --depth=1 '{REPO_URL}' {REPO_DIR} 2>&1 | tail -5",
            timeout=180, label="git clone")
    else:
        print("  Dir exists but no .git — initializing ...")
        run(c, f"cd {REPO_DIR} && git init -b {BRANCH}", label="git init")
        run(c, f"cd {REPO_DIR} && git remote add origin '{REPO_URL}'", quiet=True)
        run(c, f"cd {REPO_DIR} && git fetch origin {BRANCH} --depth=1 2>&1 | tail -5", timeout=180, label="git fetch")
        run(c, f"cd {REPO_DIR} && git reset --hard FETCH_HEAD 2>&1 | tail -3", label="git reset")
else:
    print("  .git exists — pulling latest ...")
    run(c, f"cd {REPO_DIR} && git remote set-url origin '{REPO_URL}'", quiet=True)
    run(c, f"cd {REPO_DIR} && git fetch origin {BRANCH} --depth=1 2>&1 | tail -5", timeout=180, label="git fetch")
    run(c, f"cd {REPO_DIR} && git reset --hard origin/{BRANCH} 2>&1 | tail -3", label="git reset")

run(c, f"cd {REPO_DIR} && git config credential.helper store", quiet=True)
run(c, f"echo 'https://festeraeb:{PAT}@github.com' > ~/.git-credentials && chmod 600 ~/.git-credentials", quiet=True)
print(f"  git log: {run(c, f'cd {REPO_DIR} && git log --oneline -3', quiet=True)}")

# ── 3. Python venv + deps ─────────────────────────────────────────────────────
print("\n[3/6] Setting up Python venv ...")
# Ensure python3-venv is available
venv_check = run(c, "dpkg -l python3-venv python3-pip 2>/dev/null | grep -c '^ii' || echo 0", quiet=True)
if venv_check.strip() != "2":
    print("  [apt] Installing python3-venv + python3-pip ...")
    run(c, f"echo '{PASS}' | sudo -S DEBIAN_FRONTEND=noninteractive apt-get install -y python3-venv python3-pip 2>&1 | tail -3",
        label="apt venv", timeout=120)
run(c, f"rm -rf {REPO_DIR}/.venv && python3 -m venv {REPO_DIR}/.venv 2>&1 | tail -3", label="venv create", timeout=60)
run(c, f"{REPO_DIR}/.venv/bin/pip install --upgrade pip --quiet 2>&1 | tail -2", label="pip upgrade", timeout=60)

# System GDAL dep
gdal_ver = run(c, "gdal-config --version 2>/dev/null || echo ''", quiet=True)
if not gdal_ver:
    print("  [gdal] Installing system libgdal-dev ...")
    run(c, f"echo '{PASS}' | sudo -S apt-get install -y libgdal-dev 2>&1 | tail -3", timeout=120, label="apt gdal")
    gdal_ver = run(c, "gdal-config --version", quiet=True)

print(f"\n[4/6] Installing Python deps (gdal {gdal_ver}) ...")
run(c,
    f"{REPO_DIR}/.venv/bin/pip install --quiet "
    "numpy scipy matplotlib pillow requests tqdm paramiko "
    "scikit-learn xgboost lightgbm shapely pyproj rasterio "
    "geopandas fiona simplekml fastapi uvicorn pydantic "
    "python-dotenv 2>&1 | tail -5",
    timeout=600, label="pip install core")

# Verify
print("\n[5/6] Verifying imports ...")
run(c,
    f"cd {REPO_DIR} && .venv/bin/python -c '"
    "import numpy, scipy, rasterio, sklearn, lightgbm, simplekml; "
    "print(f\"numpy {numpy.__version__}, rasterio {rasterio.__version__}, "
    "lgbm {lightgbm.__version__}, simplekml ok\")'",
    label="core imports")
run(c,
    f"cd {REPO_DIR} && .venv/bin/python -c '"
    "import torch; print(f\"torch {torch.__version__}, CUDA {torch.cuda.is_available()}, "
    "GPU: {torch.cuda.get_device_name(0) if torch.cuda.is_available() else None}\")'",
    label="torch/cuda", timeout=30)

# ── 5. Write .env ─────────────────────────────────────────────────────────────
print("\n[6/6] Writing .env ...")
env_lines = [
    f"GITHUB_PAT={PAT}",
    f"EARTHDATA_TOKEN={EARTHDATA}",
    "CESAROPS_DIR=~/wreckhunter2000-1",
    "QWEN_API_KEY=local",
    f"QWEN_BASE_URL=http://localhost:5001/v1",
]
# Copy other keys from local .env
for key in ["QWEN_API_KEY", "GROQ_API_KEY", "GEMINI_API_KEY", "ANTHROPIC_API_KEY"]:
    val = local_env.get(key, "")
    if val and val != "local":
        env_lines.append(f"{key}={val}")

env_content = "\n".join(env_lines) + "\n"
run(c, f"cat > {REPO_DIR}/.env << 'ENVEOF'\n{env_content}ENVEOF", quiet=True)
run(c, f"cat > ~/.env << 'ENVEOF'\n{env_content}ENVEOF", quiet=True)
print("  .env written")

# Create download dirs
run(c, "mkdir -p ~/wreckhunter2000-1/downloads/erie ~/wreckhunter2000-1/downloads/huron "
       "~/wreckhunter2000-1/downloads/michigan ~/wreckhunter2000-1/downloads/superior "
       "~/wreckhunter2000-1/outputs/probes ~/wreckhunter2000-1/outputs",
    quiet=True)

print(f"\n[DONE] Xeon (cesarops2) bootstrapped at {HOST}")
print("Next steps:")
print("  1. Transfer TIFs:  python scripts/_xeon_transfer_scan.py")
print("  2. Or update node_update.sh to include .129 / cesarops1 and run a deploy")
