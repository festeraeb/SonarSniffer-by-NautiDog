#!/usr/bin/env bash
# Start ZAYA1-8B HTTP API using a cu126 PyTorch env that supports sm_61 (GTX 1070).
# Uses Transformers (no vLLM). Intended: 2060 + 1070 with device_map="auto".
set -euo pipefail

VENV="${VENV:-${HOME}/.venvs/zaya-cu126}"
LOAD_MODE="${ZAYA_LOAD_MODE:-nf4}" # nf4 | fp16_auto
if [[ "$LOAD_MODE" == "fp16_auto" ]]; then
  MODEL_DIR="${ZAYA_MODEL_DIR:-${HOME}/models/Zyphra/ZAYA1-8B}"
else
  MODEL_DIR="${ZAYA_MODEL_DIR:-${HOME}/models/Zyphra/barozp-ZAYA1-8B-NF4/NF4}"
fi
HOST="${ZAYA_API_HOST:-0.0.0.0}"
PORT="${ZAYA_API_PORT:-8010}"
LOG="${ZAYA_API_LOG:-/tmp/zaya_api_cu126_2060_1070.log}"

# Physical GPUs: 0=P106, 1=2060 SUPER (sm_75), 2=1070 (sm_61)
export CUDA_DEVICE_ORDER=PCI_BUS_ID
if [[ "$LOAD_MODE" == "fp16_auto" ]]; then
  export CUDA_VISIBLE_DEVICES="${CUDA_VISIBLE_DEVICES:-1,2}"
  export ZAYA_MAX_MEMORY="${ZAYA_MAX_MEMORY:-{\"0\":\"7GiB\",\"1\":\"7GiB\",\"cpu\":\"30GiB\"}}"
else
  # NF4: single GPU (2060) — leaves headroom vs fp16_auto on both cards
  export CUDA_VISIBLE_DEVICES="${CUDA_VISIBLE_DEVICES:-1}"
fi

# Server config
export ZAYA_LOAD_MODE="$LOAD_MODE"
export ZAYA_USE_CACHE="${ZAYA_USE_CACHE:-0}"
export ZAYA_MAX_PROMPT_TOKENS="${ZAYA_MAX_PROMPT_TOKENS:-768}"
export ZAYA_MODEL_DIR="$MODEL_DIR"
export ZAYA_API_HOST="$HOST"
export ZAYA_API_PORT="$PORT"

log() { echo "[zaya-api-cu126] $*" | tee -a "$LOG"; }

if [[ ! -x "${VENV}/bin/python" ]]; then
  log "missing venv at ${VENV}"
  exit 1
fi
if [[ ! -f "${MODEL_DIR}/config.json" ]]; then
  log "missing model at ${MODEL_DIR}"
  exit 1
fi

log "venv=$VENV"
log "CUDA_VISIBLE_DEVICES=$CUDA_VISIBLE_DEVICES"
log "mode=$ZAYA_LOAD_MODE use_cache=$ZAYA_USE_CACHE"
log "max_memory=$ZAYA_MAX_MEMORY"
log "model=$MODEL_DIR"
log "listen http://${HOST}:${PORT}"
log "health: curl -s http://127.0.0.1:${PORT}/health"

exec "${VENV}/bin/python" "/mnt/t440/codebase/repos/wreckhunter2000-1/scripts/zaya/serve_zaya_api.py" 2>&1 | tee -a "$LOG"

