#!/usr/bin/env bash
# azure_vm_bootstrap.sh  —  run on a fresh Azure Ubuntu VM to set up and start a scan
# Usage: curl -fsSL <raw-url> | bash
#   or:  scp this file to the VM and run: bash azure_vm_bootstrap.sh
#
# Expects these env vars (or edit the defaults below):
#   EARTHDATA_USERNAME, EARTHDATA_PASSWORD, EARTHDATA_TOKEN, GITHUB_PAT
# ────────────────────────────────────────────────────────────────────────────

set -euo pipefail

REPO_URL="https://github.com/cesarops/wreckhunter2000-1.git"  # update if private
WORK_DIR="/home/cesarops/wreckhunter2000-1"
VENV_DIR="/home/cesarops/tpu-venv"
SCAN_SCRIPT="lake_erie_scan.py"   # change to lake_michigan_scan.py etc.
LOG_FILE="/tmp/scan_$(date +%Y%m%d_%H%M%S).log"

# ── 1. Install system deps ────────────────────────────────────────────────
echo "[1/5] Installing system packages …"
sudo apt-get update -qq
sudo apt-get install -y python3 python3-venv python3-pip git sqlite3 curl gdal-bin 2>&1 | tail -5

# ── 2. Clone repo ─────────────────────────────────────────────────────────
echo "[2/5] Cloning repo …"
if [[ -d "${WORK_DIR}/.git" ]]; then
    echo "      already cloned, pulling …"
    git -C "${WORK_DIR}" pull --ff-only
else
    git clone "${REPO_URL}" "${WORK_DIR}"
fi

# ── 3. Write .env ─────────────────────────────────────────────────────────
echo "[3/5] Writing .env …"
cat > "${WORK_DIR}/.env" <<EOF
EARTHDATA_USERNAME=${EARTHDATA_USERNAME:-cesarops.com}
EARTHDATA_PASSWORD=${EARTHDATA_PASSWORD:-}
EARTHDATA_TOKEN=${EARTHDATA_TOKEN:-}
QWEN_API_KEY=local
QWEN_MODEL=qwen2.5-coder-1.5b-instruct-q6_k
QWEN_BASE_URL=http://localhost:5001/v1
I7_TPU_PORT=5001
API_BASE_URL=http://localhost:5001
EOF

# ── 4. Python venv + deps ─────────────────────────────────────────────────
echo "[4/5] Setting up Python venv …"
python3 -m venv "${VENV_DIR}"
"${VENV_DIR}/bin/pip" install --upgrade pip -q
"${VENV_DIR}/bin/pip" install \
    fastapi uvicorn requests numpy scipy scikit-learn \
    rasterio pyproj shapely tqdm aiohttp aiofiles \
    earthaccess python-dotenv 2>&1 | tail -5

# ── 5. Init DB and start scan ─────────────────────────────────────────────
echo "[5/5] Initialising DB and starting scan → ${LOG_FILE}"
cd "${WORK_DIR}"

# Init DB if needed
if [[ ! -f db/wrecks.db ]]; then
    mkdir -p db
    "${VENV_DIR}/bin/python" init_database.py 2>&1 | tail -5 || true
fi

# Start scan in background
nohup "${VENV_DIR}/bin/python" "${SCAN_SCRIPT}" > "${LOG_FILE}" 2>&1 &
SCAN_PID=$!
echo ""
echo "================================================================"
echo "  Scan started!  PID: ${SCAN_PID}"
echo "  Log:           ${LOG_FILE}"
echo "  Watch:         tail -f ${LOG_FILE}"
echo "================================================================"
echo ""
echo "To also start the wrecks API:"
echo "  nohup ${VENV_DIR}/bin/uvicorn wrecks_api.app:app --host 0.0.0.0 --port 5001 &"
