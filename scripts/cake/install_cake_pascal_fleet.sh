#!/usr/bin/env bash
# Install native-arch cake binaries on T440 (P100 sm_60) and cesarops2 (2060 sm_75, 1070 sm_61).
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
CESAROPS2_HOST="${CESAROPS2_HOST:-10.0.0.201}"
CESAROPS2_USER="${CESAROPS2_USER:-cesarops}"
LOG="${LOG:-/tmp/install_cake_pascal_fleet.log}"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOG"; }

install_local() {
  local cap=$1
  log "T440: building cake-sm${cap//./} (CUDA_COMPUTE_CAP=$cap)…"
  bash "${REPO}/scripts/cake/install_cake_for_cap.sh" "$cap" >>"$LOG" 2>&1
}

install_remote() {
  local cap=$1
  log "cesarops2: building cake-sm${cap//./}…"
  ssh -o ConnectTimeout=15 "${CESAROPS2_USER}@${CESAROPS2_HOST}" bash -s <<REMOTE >>"$LOG" 2>&1
set -euo pipefail
REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"
[[ -d "\$REPO" ]] || REPO="/codebase/repos/wreckhunter2000-1"
export CUDA_COMPUTE_CAP=$cap
bash "\$REPO/scripts/cake/install_cake_for_cap.sh" "$cap"
REMOTE
}

: >"$LOG"
log "=== Pascal/Turing cake fleet install ==="

install_local 60
install_remote 75
install_remote 61

log "=== Done — binaries ==="
ls -la /opt/cesarops/cake/bin/cake-sm* 2>/dev/null | tee -a "$LOG" || true
ssh "${CESAROPS2_USER}@${CESAROPS2_HOST}" 'ls -la /opt/cesarops/cake/bin/cake-sm* 2>/dev/null' | tee -a "$LOG" || true
log "log: $LOG"
