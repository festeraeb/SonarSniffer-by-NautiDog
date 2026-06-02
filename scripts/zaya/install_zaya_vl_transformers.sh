#!/usr/bin/env bash
# ZAYA1-VL-8B uses Zyphra transformers (zaya1-vl), not vLLM. Install for vision inference later.
set -euo pipefail

VENV="${VENV:-${HOME}/.venvs/zaya-vl}"
LOG="${LOG:-/tmp/install_zaya_vl.log}"

log() { echo "[zaya-vl-install] $*" | tee -a "$LOG"; }

: >"$LOG"
if [[ ! -d "$VENV" ]]; then
  python3 -m venv "$VENV"
fi
# shellcheck source=/dev/null
source "${VENV}/bin/activate"
pip install -U pip wheel

log "transformers zaya1-vl + qwen-vl-utils (flash-attn may build long or skip on Pascal)…"
pip install "transformers[dev-torch] @ git+https://github.com/Zyphra/transformers.git@zaya1-vl" \
  qwen-vl-utils==0.0.2 >>"$LOG" 2>&1 || {
  log "retry without flash_attn requirement…"
  pip install "transformers @ git+https://github.com/Zyphra/transformers.git@zaya1-vl" \
    qwen-vl-utils==0.0.2 torch torchvision >>"$LOG" 2>&1
}

log "done — use ${VENV} for ZAYA1-VL-8B; weights: ~/models/Zyphra/ZAYA1-VL-8B"
