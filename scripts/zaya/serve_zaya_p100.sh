#!/usr/bin/env bash
# ZAYA1-8B on 2× Tesla P100 (Pascal sm_60) via Zyphra transformers@zaya1.
# vLLM does not build on Pascal — use Transformers only.
#
# Modes (ZAYA_LOAD_MODE):
#   bnb_nf4   — NF4+double-quant on GPU0 (default; reliable on Pascal)
#   ZAYA_BNB_DEVICE_MAP=auto ZAYA_LOAD_MODE=bnb_nf4 — try 2-GPU shard (experimental)
#   fp16_auto — full fp16 shard across both cards
#   nf4       — legacy barozp pre-quant (not recommended)
#
# Sampling (Zyphra): math/reasoning temp=1.0 top_p=0.95; coding use ZAYA_DEFAULT_TEMP=0.6
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
VENV="${VENV:-/data/cesarops/venvs/zaya-p100}"
MODEL="${MODEL:-/data/cesarops/zaya_models/ZAYA1-8B}"
PORT="${PORT:-5001}"
LOG="${LOG:-/data/cesarops/logs/zaya-p100-${PORT}.log}"
FREE_PORTS="${FREE_PORTS:-1}"

log() { echo "[zaya-p100] $*" | tee -a "$LOG"; }

mkdir -p "$(dirname "$LOG")"

if [[ "$FREE_PORTS" == "1" ]]; then
  log "stopping prior listeners on :5001/:5002…"
  pkill -f 'serve_zaya_api.py' 2>/dev/null || true
  pkill -f 'vllm serve' 2>/dev/null || true
  fuser -k 5001/tcp 5002/tcp 2>/dev/null || true
  sleep 2
fi

if [[ ! -x "${VENV}/bin/python" ]]; then
  log "run: bash scripts/zaya/install_zaya_transformers_p100.sh"
  exit 1
fi
if [[ ! -f "${MODEL}/config.json" ]]; then
  log "missing weights at ${MODEL}"
  exit 1
fi

export CUDA_DEVICE_ORDER=PCI_BUS_ID
# Default: one P100 for NF4 (~6GB). For fp16_auto across both: CUDA_VISIBLE_DEVICES=0,1 ZAYA_LOAD_MODE=fp16_auto
export CUDA_VISIBLE_DEVICES="${CUDA_VISIBLE_DEVICES:-0}"

export ZAYA_LOAD_MODE="${ZAYA_LOAD_MODE:-bnb_nf4}"
export ZAYA_MODEL_DIR="$MODEL"
export ZAYA_BNB_DEVICE_MAP="${ZAYA_BNB_DEVICE_MAP:-cuda:0}"
export ZAYA_MAX_MEMORY="${ZAYA_MAX_MEMORY:-}"
# Required for Zaya CCA on Transformers (use_cache=False breaks decode)
export ZAYA_USE_CACHE=1
export ZAYA_MAX_PROMPT_TOKENS="${ZAYA_MAX_PROMPT_TOKENS:-768}"
export ZAYA_MAX_NEW_TOKENS="${ZAYA_MAX_NEW_TOKENS:-256}"
export ZAYA_DEFAULT_TEMP="${ZAYA_DEFAULT_TEMP:-1.0}"
export ZAYA_TOP_P="${ZAYA_TOP_P:-0.95}"
export ZAYA_TOP_K="${ZAYA_TOP_K:--1}"
export ZAYA_API_HOST=0.0.0.0
export ZAYA_API_PORT="$PORT"

log "Pascal Transformers serve mode=$ZAYA_LOAD_MODE port=$PORT"
log "CUDA_VISIBLE_DEVICES=$CUDA_VISIBLE_DEVICES temp=$ZAYA_DEFAULT_TEMP top_p=$ZAYA_TOP_P"
log "model=$MODEL"

exec "${VENV}/bin/python" "${REPO}/scripts/zaya/serve_zaya_api.py" 2>&1 | tee -a "$LOG"
