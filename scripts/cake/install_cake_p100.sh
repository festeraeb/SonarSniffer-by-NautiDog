#!/usr/bin/env bash
# P100 (sm_60): native candle-kernels build fails (needs sm_70+ WMMA). Cake 72B uses CPU for layers 30-79 on T440 RAM.
# Optional: try sm_70 build for experiments — still may not run on P100 at inference time.
set -euo pipefail
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
INSTALL_ROOT="${INSTALL_ROOT:-/opt/cesarops/cake}"
export CUDA_COMPUTE_CAP="${CUDA_COMPUTE_CAP:-70}"
export CAKE_FEATURES="${CAKE_FEATURES:-cuda}"
log() { echo "[install_cake_p100] $*"; }
log "NOTE: sm_60 build unsupported; trying CUDA_COMPUTE_CAP=${CUDA_COMPUTE_CAP} (fleet uses CPU for P100#1 layers)"
if ! bash "${REPO}/scripts/install_cake_fleet.sh"; then
  log "Build failed — keep topology_fleet_hetero_70b.yml (t440_ram layers 30-79, no P100 CUDA worker)"
  exit 1
fi
BIN="${HOME}/.cargo/bin/cake"
[[ -x "$BIN" ]] || BIN="${INSTALL_ROOT}/bin/cake"
sudo mkdir -p "${INSTALL_ROOT}/bin"
sudo ln -sf "$BIN" "${INSTALL_ROOT}/bin/cake-p100"
log "Linked cake-p100 — 72B fleet still uses CPU RAM worker for ex-P100 layers unless you verify CUDA on P100"
