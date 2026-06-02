#!/usr/bin/env bash
# Zyphra transformers@zaya1 + deps for Pascal P100 (no stock transformers).
set -euo pipefail

VENV="${VENV:-/data/cesarops/venvs/zaya-p100}"
LOG="${LOG:-/data/cesarops/logs/install_zaya_transformers_p100.log}"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOG"; }

mkdir -p "$(dirname "$LOG")"
# shellcheck source=/dev/null
source "${VENV}/bin/activate"
pip install -U pip wheel
log "installing transformers@zaya1 + accelerate + bitsandbytes…"
pip install "transformers @ git+https://github.com/Zyphra/transformers.git@zaya1" \
  accelerate bitsandbytes fastapi uvicorn 2>&1 | tee -a "$LOG"
log "pin huggingface-hub for transformers 4.57.x"
pip install 'huggingface-hub>=0.34.0,<1.0' 2>&1 | tee -a "$LOG"
log "done"
