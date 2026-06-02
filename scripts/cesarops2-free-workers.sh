#!/usr/bin/env bash
# Free cesarops2 GPU/RAM workers (llama, Cake, Forge, Nauti) — T440 untouched when isolated.
#
# Usage:
#   bash scripts/cesarops2-free-workers.sh
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2-isolated.env" ]] && source "$REPO/scripts/cesarops2-isolated.env"
[[ -f "${HOME}/.cache/cesarops/cesarops2-isolated" ]] && source "$REPO/scripts/cesarops2-isolated.env" 2>/dev/null || true

log() { echo "[c2-free] $*"; }

log "stopping local workers (T440 not contacted)"
bash "$REPO/scripts/cesarops2_qwen36_gemma4_dual.sh" stop 2>/dev/null || true
bash "$REPO/scripts/cesarops2_coder_rust_dual.sh" stop 2>/dev/null || true
bash "$REPO/scripts/cake/start-cake-c2-hybrid.sh" stop 2>/dev/null || true
bash "$REPO/scripts/cake/stop-fleet-cluster.sh" 2>/dev/null || true
bash "$REPO/scripts/nauti-inferer-c2.sh" stop 2>/dev/null || true
bash "$REPO/scripts/cesarops2-forge-edge.sh" stop 2>/dev/null || true
pkill -f 'llama-server.*(5200|5571|5001|5002)' 2>/dev/null || true
pkill -f 'cake run' 2>/dev/null || true
sleep 1

log "GPU:"
nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv 2>/dev/null || true
if ps aux | grep -E 'llama-server|cake run' | grep -v grep >/dev/null; then
  log "WARN: some processes still running"
  ps aux | grep -E 'llama-server|cake run' | grep -v grep
else
  log "all c2 inference workers stopped — ready for beta"
fi
