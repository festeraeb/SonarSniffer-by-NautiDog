#!/usr/bin/env bash
# Serve ZAYA1-8B on RTX 2060 (GPU 1) + GTX 1070 (GPU 2) via DP + expert parallel.
# Official Zyphra layout: vLLM zaya1-pr with -dp N --enable-expert-parallel (not TP).
set -euo pipefail

VENV="${VENV:-${HOME}/.venvs/zaya-vllm}"
MODEL="${MODEL:-${HOME}/models/Zyphra/ZAYA1-8B}"
PORT="${PORT:-8010}"
# Physical GPUs: 0=P106 (unused), 1=2060 Turing, 2=1070 Pascal
export CUDA_VISIBLE_DEVICES="${CUDA_VISIBLE_DEVICES:-1,2}"
FREE_LLAMA="${FREE_LLAMA:-0}"

log() { echo "[zaya-serve] $*"; }

if [[ ! -x "${VENV}/bin/vllm" ]]; then
  log "missing vLLM — run: bash scripts/zaya/install_zaya_vllm_c2.sh"
  exit 1
fi
if [[ ! -f "${MODEL}/config.json" ]]; then
  log "missing weights — run: bash scripts/zaya/download_zaya_models.sh"
  exit 1
fi

if [[ "$FREE_LLAMA" == "1" ]]; then
  log "stopping llama-server on c2 GPUs (frees VRAM on 2060/1070)…"
  pkill -f 'llama-server.*-dev CUDA' 2>/dev/null || true
  sleep 3
fi

# shellcheck source=/dev/null
source "${VENV}/bin/activate"

log "GPUs: CUDA_VISIBLE_DEVICES=$CUDA_VISIBLE_DEVICES (2060 + 1070)"
log "model=$MODEL port=$PORT dp=2 ep"
log "flags: bf16, mamba-cache float32, qwen3 reasoning, zaya_xml tools"

exec vllm serve "$MODEL" \
  --host 0.0.0.0 \
  --port "$PORT" \
  --dtype bfloat16 \
  --mamba-cache-dtype float32 \
  --reasoning-parser qwen3 \
  --enable-auto-tool-choice \
  --tool-call-parser zaya_xml \
  --data-parallel-size 2 \
  --enable-expert-parallel
