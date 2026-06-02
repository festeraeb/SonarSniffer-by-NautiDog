#!/usr/bin/env bash
# P100 inference lifecycle — llama-server + draft-mtp (not Kobold).
#
# Usage:
#   bash scripts/p100_cycle.sh free     # stop :5001/:5002
#   bash scripts/p100_cycle.sh restore  # llama-server MTP on both P100s
#   bash scripts/p100_cycle.sh status
set -euo pipefail

ACTION="${1:-status}"
LLAMA="${LLAMA:-/home/cesarops/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA="${LLAMA:-/home/cesarops/bin/llama-server}"

MODEL_CODER="${MODEL_CODER:-/codebase/models/Qwen3.6-35B-A3B-Q4_K_M.gguf}"
MODEL_REVIEWER="${MODEL_REVIEWER:-/data/cesarops/local_models/Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf}"
CTX="${CTX:-8192}"
NGL_CODER="${NGL_CODER:-36}"
NGL_REVIEWER="${NGL_REVIEWER:-99}"
MTP_DRAFT_MAX="${MTP_DRAFT_MAX:-2}"

LOG="${LOG:-/tmp/p100-llama-restore.log}"

free_ports() {
  for port in 5001 5002; do
    pkill -9 -f "koboldcpp.*--port ${port}" 2>/dev/null || true
    pkill -9 -f "llama-server.*--port ${port}" 2>/dev/null || true
    pkill -9 -f "llama-server.*-port ${port}" 2>/dev/null || true
    fuser -k "${port}/tcp" 2>/dev/null || true
  done
  sleep 2
  echo "[p100_cycle] ports 5001/5002 cleared"
  nvidia-smi --query-gpu=index,memory.used --format=csv 2>/dev/null || true
}

start_llama() {
  local model=$1 port=$2 gpu=$3 tag=$4 ngl=$5 use_mtp=${6:-0}
  if [[ ! -f "$model" ]]; then
    echo "[p100_cycle] SKIP $tag — missing $model"
    return 1
  fi
  if [[ ! -x "$LLAMA" ]]; then
    echo "[p100_cycle] llama-server not found: $LLAMA"
    return 1
  fi
  local mtp_args=()
  if [[ "$use_mtp" == "1" ]] || [[ "$model" == *MTP* ]]; then
    mtp_args=(--spec-type draft-mtp --spec-draft-n-max "$MTP_DRAFT_MAX")
  fi
  local layer_args=(-ngl "$ngl")
  if [[ "${LLAMA_FIT_ON:-0}" == "1" ]] || [[ "$model" == *MoE* ]] || [[ "$model" == *Qwen3.6* ]]; then
    layer_args=(--fit on)
  fi
  CUDA_VISIBLE_DEVICES="$gpu" setsid "$LLAMA" \
    -m "$model" \
    --host 0.0.0.0 \
    --port "$port" \
    -dev CUDA0 \
    -sm layer \
    "${layer_args[@]}" \
    -c "$CTX" \
    -t 4 \
    "${mtp_args[@]}" \
    >>"$LOG" 2>&1 &
  echo "[p100_cycle] $tag port=$port GPU=$gpu mtp=${mtp_args[*]:-off} PID=$!"
}

restore_llama() {
  free_ports
  : >"$LOG"
  if [[ "${P100_CODER_ONLY:-0}" == "1" ]]; then
    # P100#1 may be on Cake — only bring up coder MoE on #0 (:5001)
    start_llama "$MODEL_CODER" 5001 0 "coder-qwen-moe-p1000" "$NGL_CODER" 0
    echo "[p100_cycle] coder-only (Qwen MoE :5001) log=$LOG"
    return
  fi
  # Dual-P100: reviewer MTP on #1, Qwen MoE coder on #0
  start_llama "$MODEL_REVIEWER" 5002 1 "reviewer-mtp-p1001" "$NGL_REVIEWER" 1
  sleep 8
  start_llama "$MODEL_CODER" 5001 0 "coder-qwen-moe-p1000" "$NGL_CODER" 0
  echo "[p100_cycle] llama-server MTP log=$LOG"
  echo "[p100_cycle] Forge MTP pool: cesarops2 :5571/:5200 first, then :5002/:5001"
}

case "$ACTION" in
  free)    free_ports ;;
  restore) restore_llama ;;
  status)
    pgrep -af 'llama-server.*500[12]' || echo "no llama-server on 5001/5002"
    pgrep -af 'koboldcpp.*500[12]' || echo "no kobold on 5001/5002"
    for p in 5001 5002; do
      curl -sf --max-time 2 "http://127.0.0.1:${p}/v1/models" | head -c 120 && echo " (:$p)" || echo ":$p down"
    done
    nvidia-smi --query-gpu=index,name,memory.used --format=csv 2>/dev/null || true
    ;;
  *)
    echo "Usage: $0 {free|restore|status}"
    exit 1
    ;;
esac
