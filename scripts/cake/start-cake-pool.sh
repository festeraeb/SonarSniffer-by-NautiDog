#!/usr/bin/env bash
# Start Cake 70B cluster with P100#1 in the VRAM shard pool (:8081 only). Free :5002 if needed.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "[cake-pool] P100#0 stays on Gemma :5001 — not touched"
pkill -f 'llama-server.*--port 5002' 2>/dev/null || true
pkill -f 'llama-server.*-port 5002' 2>/dev/null || true
fuser -k 5002/tcp 2>/dev/null || true
sleep 1

bash "${SCRIPT_DIR}/start-fleet-hetero-70b.sh"

echo "[cake-pool] API http://127.0.0.1:8081 (P100#1 + RAM + c2 RTX in cluster VRAM pool)"
