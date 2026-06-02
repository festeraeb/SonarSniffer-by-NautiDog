#!/usr/bin/env bash
# Start ZAYA1-8B API on RTX 2060 (or 2060+1070 via fp16_auto).
# No vLLM — Transformers only.
set -euo pipefail

VENV="${VENV:-${HOME}/.venvs/zaya-vllm}"
LOAD_MODE="${ZAYA_LOAD_MODE:-fp16_auto}" # fp16_auto | nf4
if [[ "$LOAD_MODE" == "fp16_auto" ]]; then
  MODEL_DIR="${ZAYA_MODEL_DIR:-${HOME}/models/Zyphra/ZAYA1-8B}"
else
  MODEL_DIR="${ZAYA_MODEL_DIR:-${HOME}/models/Zyphra/barozp-ZAYA1-8B-NF4/NF4}"
fi
HOST="${ZAYA_API_HOST:-0.0.0.0}"
PORT="${ZAYA_API_PORT:-8010}"
LOG="${ZAYA_API_LOG:-/tmp/zaya_api_2060.log}"

# PCI: 0=P106, 1=2060 SUPER, 2=1070
export CUDA_DEVICE_ORDER=PCI_BUS_ID
if [[ "$LOAD_MODE" == "fp16_auto" ]]; then
  # two-GPU pipeline split inside transformers (local cuda indices 0,1)
  export CUDA_VISIBLE_DEVICES="${CUDA_VISIBLE_DEVICES:-1,2}"
  export ZAYA_MAX_MEMORY="${ZAYA_MAX_MEMORY:-{\"0\":\"7GiB\",\"1\":\"7GiB\",\"cpu\":\"30GiB\"}}"
  export ZAYA_USE_CACHE="${ZAYA_USE_CACHE:-0}"
else
  export CUDA_VISIBLE_DEVICES="${CUDA_VISIBLE_DEVICES:-1}"
  export ZAYA_USE_CACHE="${ZAYA_USE_CACHE:-0}"
fi

log() { echo "[zaya-api] $*" | tee -a "$LOG"; }

if [[ ! -x "${VENV}/bin/python" ]]; then
  log "missing venv — run: bash scripts/zaya/install_zaya_vllm_c2.sh"
  exit 1
fi
if [[ ! -f "${MODEL_DIR}/config.json" ]]; then
  log "missing model at ${MODEL_DIR}"
  if [[ "$LOAD_MODE" == "fp16_auto" ]]; then
    log "download: hf download Zyphra/ZAYA1-8B --local-dir ${HOME}/models/Zyphra/ZAYA1-8B"
  else
    log "download: hf download barozp/ZAYA1-8B-BNB --include 'NF4/*' --local-dir ${HOME}/models/Zyphra/barozp-ZAYA1-8B-NF4"
  fi
  exit 1
fi

# shellcheck source=/dev/null
source "${VENV}/bin/activate"

"${VENV}/bin/pip" install -q fastapi uvicorn accelerate bitsandbytes 2>/dev/null || true

export ZAYA_MODEL_DIR="$MODEL_DIR"
export ZAYA_API_HOST="$HOST"
export ZAYA_API_PORT="$PORT"
export ZAYA_LOAD_MODE="$LOAD_MODE"

log "mode=$LOAD_MODE"
log "CUDA_VISIBLE_DEVICES=$CUDA_VISIBLE_DEVICES"
log "model=$MODEL_DIR"
if [[ "$LOAD_MODE" == "fp16_auto" ]]; then
  log "max_memory=$ZAYA_MAX_MEMORY"
fi
log "listen http://${HOST}:${PORT}"
log "health: curl -s http://127.0.0.1:${PORT}/health"
log "chat:   curl -s http://127.0.0.1:${PORT}/v1/chat/completions -H 'Content-Type: application/json' -d '{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}'"

exec "${VENV}/bin/python" "$(dirname "$0")/serve_zaya_api.py" 2>&1 | tee -a "$LOG"
