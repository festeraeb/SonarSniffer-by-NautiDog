#!/usr/bin/env bash
# Dual P100 for dual-lane Forge:
#   :5001 P100 — Gemma-4 MoE (Lane A coder)
#   :5002 P100 — Qwen2.5-Coder-14B (Lane B coder)
#   :5010 CPU — Qwen3.6 MoE think/polish (scripts/start_qwen36_moe_cpu_laneb.sh)
#   Full T440 bring-up: scripts/t440_dual_lane_layout.sh
#
# Usage:
#   bash scripts/p100_gemma_r1_dual.sh          # start both
#   bash scripts/p100_gemma_r1_dual.sh free     # stop :5001/:5002 + Zaya
#   bash scripts/p100_gemma_r1_dual.sh status
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
LLAMA="${LLAMA:-/home/cesarops/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA="/home/cesarops/bin/llama-server"

MODEL_CODER="${MODEL_CODER:-}"
MODEL_REVIEWER="${MODEL_REVIEWER:-}"
CTX_CODER="${CTX_CODER:-8192}"
CTX_REVIEWER="${CTX_REVIEWER:-4096}"
LOG="${LOG:-/data/cesarops/logs/p100-gemma-r1-dual.log}"

ACTION="${1:-start}"

log() { echo "[p100-gemma-r1] $*" | tee -a "$LOG"; }

resolve_model() {
  local explicit="$1"
  shift
  if [[ -n "$explicit" && -f "$explicit" ]]; then
    echo "$explicit"
    return 0
  fi
  for cand in "$@"; do
    if [[ -f "$cand" ]]; then
      echo "$cand"
      return 0
    fi
  done
  return 1
}

free_ports() {
  pkill -f 'serve_zaya_api.py' 2>/dev/null || true
  for port in 5001 5002; do
    pkill -9 -f "koboldcpp.*--port ${port}" 2>/dev/null || true
    pkill -9 -f "llama-server.*--port ${port}" 2>/dev/null || true
    pkill -9 -f "llama-server.*-port ${port}" 2>/dev/null || true
    fuser -k "${port}/tcp" 2>/dev/null || true
  done
  sleep 2
  log "ports 5001/5002 cleared"
  nvidia-smi --query-gpu=index,memory.used --format=csv 2>/dev/null || true
}

start_llama() {
  local model=$1 port=$2 gpu=$3 tag=$4 ctx=$5
  local reasoning="${6:-auto}"
  if [[ ! -f "$model" ]]; then
    log "SKIP $tag — missing $model"
    return 1
  fi
  if [[ ! -x "$LLAMA" ]]; then
    log "llama-server not found: $LLAMA"
    return 1
  fi
  # --fit on (no -ngl): auto-fit largest model to one 16GB P100
  # Gemma coder: --reasoning off → answers in message.content for Forge chat API
  # R1 reviewer: --reasoning on (default) for review/correction passes
  CUDA_VISIBLE_DEVICES="$gpu" setsid "$LLAMA" \
    -m "$model" \
    --host 0.0.0.0 \
    --port "$port" \
    -dev CUDA0 \
    -sm layer \
    --fit on \
    --reasoning "$reasoning" \
    -c "$ctx" \
    -t 4 \
    >>"$LOG" 2>&1 &
  log "$tag port=$port GPU=$gpu reasoning=$reasoning PID=$!"
}

wait_ready() {
  local port=$1 label=$2
  for i in $(seq 1 90); do
    if curl -sf --max-time 3 "http://127.0.0.1:${port}/v1/models" >/dev/null 2>&1; then
      log "$label ready (${i}×5s)"
      return 0
    fi
    sleep 5
  done
  log "WARN: $label :$port not ready — see $LOG"
  return 1
}

start_dual() {
  mkdir -p "$(dirname "$LOG")"
  free_ports
  : >"$LOG"

  local gemma_model
  gemma_model="$(resolve_model "$MODEL_CODER" \
    /codebase/models/Gemma-4-26B-MoE-IQ4_XS.gguf \
    /mnt/t440/codebase/models/Gemma-4-26B-MoE-IQ4_XS.gguf \
    /mnt/t440/models/Gemma-4-26B-MoE-IQ4_XS.gguf)" || {
      log "SKIP gemma4-coder — no Gemma-4-26B-MoE model found"
      return 1
    }

  start_llama "$gemma_model" 5001 1 "gemma4-coder" "$CTX_CODER" "off"
  wait_ready 5001 "Gemma-4-26B" || true

  log "Lane B coder: Qwen14 on :5002 (P100 GPU0)"
  bash "${REPO}/scripts/start_qwen14_coder_p100.sh" || log "WARN Qwen14 :5002 failed"
  log "Lane B think/polish: start CPU MoE — bash scripts/start_qwen36_moe_cpu_laneb.sh"
  log "Forge Lane A: http://127.0.0.1:5001  Lane B coder: http://127.0.0.1:5002"
}

case "$ACTION" in
  free|stop) free_ports ;;
  status)
    pgrep -af 'llama-server.*500[12]' || echo "no llama on 5001/5002"
    for p in 5001 5002; do
      curl -sf --max-time 2 "http://127.0.0.1:${p}/v1/models" | head -c 160 && echo " (:$p)" || echo ":$p down"
    done
    nvidia-smi --query-gpu=index,name,memory.used --format=csv 2>/dev/null || true
    ;;
  start|"") start_dual ;;
  *)
    echo "Usage: $0 {start|free|status}"
    exit 1
    ;;
esac
