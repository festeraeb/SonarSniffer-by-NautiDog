#!/bin/bash
# Run on cesarops2 — TinyLlama validator on P1000 (port 5571), llama-server.
set -u

SHARE_MOUNT="/mnt/cesarops-models"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MODEL="${SHARE_MOUNT}/TinyLlama-1.1B-Chat-v1.0-Q4_K_M.gguf"

if [[ ! -f "$MODEL" ]]; then
  MODEL="/mnt/storage/models/TinyLlama-1.1B-Chat-v1.0-Q4_K_M.gguf"
fi

if [[ ! -f "$MODEL" ]]; then
  echo "TinyLlama model not found"; exit 1
fi

MODEL="$MODEL" PORT=5571 DEV=CUDA1 CTX=2048 NGL=99 THREADS=2 \
  LOG=/tmp/llama-picasso.log \
  bash "$SCRIPT_DIR/launch_llama_remote.sh"
