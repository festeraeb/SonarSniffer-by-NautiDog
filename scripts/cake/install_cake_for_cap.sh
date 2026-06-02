#!/usr/bin/env bash
# Build cake-cli for a specific NVIDIA compute capability (native PTX, no sm_86 JIT on Pascal/Turing).
# Usage: bash scripts/cake/install_cake_for_cap.sh 60   # → /opt/cesarops/cake/bin/cake-sm60
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"
INSTALL_ROOT="${INSTALL_ROOT:-/opt/cesarops/cake}"
CAP="${1:-}"
if [[ -z "$CAP" ]]; then
  echo "usage: $0 <compute_cap>   e.g. 60 61 75" >&2
  exit 1
fi

export CUDA_COMPUTE_CAP="$CAP"
export CAKE_FEATURES="${CAKE_FEATURES:-cuda,master,qwen2}"
export CAKE_NO_DEFAULT_FEATURES="${CAKE_NO_DEFAULT_FEATURES:-1}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${HOME}/.cache/cake-cargo-sm${CAP//./}}"
export CAKE_INSTALL_FORCE=1
BIN_NAME="cake-sm${CAP//./}"

# Pascal/Turing: WMMA MoE CUDA sources fail nvcc; use gguf-only MoE + native PTX arch.
if [[ "$CAP" == "60" || "$CAP" == "61" || "$CAP" == "75" ]]; then
  export CANDLE_PASCAL_MOE_GGUF_ONLY=1
fi

log() { echo "[install_cake_${BIN_NAME}] $*"; }

log "CUDA_COMPUTE_CAP=$CUDA_COMPUTE_CAP CARGO_TARGET_DIR=$CARGO_TARGET_DIR → ${INSTALL_ROOT}/bin/${BIN_NAME}"
bash "${SCRIPT_DIR}/patch_candle_kernels_pascal.sh" "$CAP"
bash "${REPO}/scripts/install_cake_fleet.sh"

# Each cap must be its own file — cargo install always writes ~/.cargo/bin/cake (last build wins).
BUILT="${CARGO_TARGET_DIR}/release/cake"
[[ -x "$BUILT" ]] || BUILT="${HOME}/.cargo/bin/cake"
[[ -x "$BUILT" ]] || { log "ERROR: cake binary missing after install"; exit 1; }

sudo mkdir -p "${INSTALL_ROOT}/bin"
sudo install -m 0755 "$BUILT" "${INSTALL_ROOT}/bin/${BIN_NAME}"
log "Installed ${INSTALL_ROOT}/bin/${BIN_NAME} from $BUILT"
"${INSTALL_ROOT}/bin/${BIN_NAME}" --version 2>/dev/null || true
strings "${INSTALL_ROOT}/bin/${BIN_NAME}" 2>/dev/null | grep -E '^\.target sm_' | sort -u | head -5 || true
