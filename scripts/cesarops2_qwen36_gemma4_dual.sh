#!/usr/bin/env bash
# cesarops2: Qwen3.6-35B-A3B MoE on RTX 2060 + P106 (:5200), Gemma-4-E4B on GTX 1070 (:5571).
#
# llama device order (not nvidia-smi index):
#   CUDA0 = RTX 2060 SUPER (8GB)
#   CUDA1 = GTX 1070 (8GB) — Gemma only
#   CUDA2 = P106-100 (6GB) — extra Qwen layers when USE_P106=1
#
# Turbo quant: prefer Qwen3.6-35B-A3B-MXFP4_MOE.gguf (MXFP4 MoE) over Q4_K_M.
#
# Usage:
#   bash scripts/cesarops2_qwen36_gemma4_dual.sh start
#   USE_P106=0 bash scripts/cesarops2_qwen36_gemma4_dual.sh start   # 2060 only
#   bash scripts/cesarops2_qwen36_gemma4_dual.sh status
#   bash scripts/cesarops2_qwen36_gemma4_dual.sh stop
set -euo pipefail
REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
[[ -f "$REPO_ROOT/scripts/cesarops2-isolated.env" ]] && source "$REPO_ROOT/scripts/cesarops2-isolated.env"
[[ -f "${HOME}/.cache/cesarops/cesarops2-isolated" ]] && source "$REPO_ROOT/scripts/cesarops2-isolated.env" 2>/dev/null || true

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2_qwen36_gemma4_dual.sh" ]] || REPO="/codebase/repos/wreckhunter2000-1"
MODELS="${MODELS:-/mnt/t440/models}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA=/home/cesarops/bin/llama-server
PID_DIR="${PID_DIR:-/tmp/cesarops2-qwen36-gemma4}"

# Turbo MoE quant first; Q4_K_M fallback
MODEL_QWEN_TURBO="${MODEL_QWEN_TURBO:-$MODELS/Qwen3.6-35B-A3B-MXFP4_MOE.gguf}"
MODEL_QWEN_FALLBACK="${MODEL_QWEN_FALLBACK:-$MODELS/Qwen3.6-35B-A3B-Q4_K_M.gguf}"
if [[ -f "${MODEL_QWEN:-}" ]]; then
  :
elif [[ -f "$MODEL_QWEN_TURBO" ]]; then
  MODEL_QWEN="$MODEL_QWEN_TURBO"
else
  MODEL_QWEN="$MODEL_QWEN_FALLBACK"
fi

MODEL_GEMMA="${MODEL_GEMMA:-$MODELS/gemma-4-E4B-it-Q4_K_M.gguf}"
PORT_QWEN="${PORT_QWEN:-5200}"
PORT_GEMMA="${PORT_GEMMA:-5571}"
DEV_GEMMA="${DEV_GEMMA:-CUDA1}"
CTX_QWEN="${CTX_QWEN:-3072}"
CTX_GEMMA="${CTX_GEMMA:-4096}"
NGL_GEMMA="${NGL_GEMMA:-99}"

# Split Qwen: 2060 primary + P106 spill (frees 1070 for Gemma only)
USE_P106="${USE_P106:-1}"
DEV_QWEN_2060="${DEV_QWEN_2060:-CUDA0}"
DEV_QWEN_P106="${DEV_QWEN_P106:-CUDA2}"
# --fit margins MiB per device (2060, P106) — do not pass -ts with --fit (llama aborts)
FIT_TARGET_QWEN="${FIT_TARGET_QWEN:-896,640}"

QWEN_FIT="${QWEN_FIT:-on}"
# Turbo inference: quant KV + flash-attn auto + repack (default on)
FA_QWEN="${FA_QWEN:-auto}"
FA_GEMMA="${FA_GEMMA:-off}"
KV_K_QWEN="${KV_K_QWEN:-q8_0}"
KV_V_QWEN="${KV_V_QWEN:-q8_0}"
# 1070 (Pascal): KV quant needs flash-attn; keep f16 KV for Gemma
KV_K_GEMMA="${KV_K_GEMMA:-f16}"
KV_V_GEMMA="${KV_V_GEMMA:-f16}"
UBATCH="${UBATCH:-384}"
# MXFP4 + mmap overrides can be slow; no-mmap helps layer fit on multi-GPU
QWEN_MMAP="${QWEN_MMAP:-off}"

log() { echo "[qwen36-gemma4] $*"; }

qwen_dev_args() {
  if [[ "$USE_P106" == "1" ]]; then
    echo "-dev ${DEV_QWEN_2060},${DEV_QWEN_P106}"
  else
    echo "-dev ${DEV_QWEN_2060}"
  fi
}

qwen_extra_args() {
  local -a args=(-sm layer --fit "$QWEN_FIT" -fa "$FA_QWEN" -ctk "$KV_K_QWEN" -ctv "$KV_V_QWEN" -ub "$UBATCH")
  if [[ "$QWEN_MMAP" == "off" ]]; then
    args+=(--no-mmap)
  fi
  if [[ "$USE_P106" == "1" ]]; then
    args+=(--fit-target "$FIT_TARGET_QWEN")
  else
    args+=(--fit-target "${FIT_TARGET_QWEN%%,*}")
  fi
  printf '%s\n' "${args[@]}"
}

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  pkill -f "llama-server.*-port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}

stop_all_llm() {
  pkill -f 'serve_zaya_api.py' 2>/dev/null || true
  pkill -f dispatch_bgrade_enhance.py 2>/dev/null || true
  bash "$REPO/scripts/cesarops2_coder_rust_dual.sh" stop 2>/dev/null || true
  stop_port "$PORT_QWEN"
  stop_port "$PORT_GEMMA"
  sleep 2
}

start_qwen() {
  mkdir -p "$PID_DIR"
  if [[ ! -f "$MODEL_QWEN" ]]; then
    log "ERROR: missing $MODEL_QWEN"
    return 1
  fi
  stop_port "$PORT_QWEN"
  local logf="$PID_DIR/llama-${PORT_QWEN}.log"
  local -a dev_args extra_args
  read -ra dev_args <<< "$(qwen_dev_args)"
  mapfile -t extra_args < <(qwen_extra_args)

  log "Qwen model=$(basename "$MODEL_QWEN") dev=${dev_args[*]} p106=$USE_P106 fit_target=$FIT_TARGET_QWEN"
  setsid "$LLAMA" \
    -m "$MODEL_QWEN" \
    --host 0.0.0.0 \
    --port "$PORT_QWEN" \
    "${dev_args[@]}" \
    "${extra_args[@]}" \
    -c "$CTX_QWEN" \
    -t 4 \
    -np 1 \
    </dev/null >>"$logf" 2>&1 &
  disown 2>/dev/null || true
  echo $! >"$PID_DIR/llama-${PORT_QWEN}.pid"
  log "Qwen port=$PORT_QWEN fit=$QWEN_FIT ctx=$CTX_QWEN kv=${KV_K_QWEN}/${KV_V_QWEN} pid=$(cat "$PID_DIR/llama-${PORT_QWEN}.pid")"
}

start_gemma() {
  mkdir -p "$PID_DIR"
  if [[ ! -f "$MODEL_GEMMA" ]]; then
    log "ERROR: missing $MODEL_GEMMA"
    return 1
  fi
  stop_port "$PORT_GEMMA"
  local logf="$PID_DIR/llama-${PORT_GEMMA}.log"
  setsid "$LLAMA" \
    -m "$MODEL_GEMMA" \
    --host 0.0.0.0 \
    --port "$PORT_GEMMA" \
    -dev "$DEV_GEMMA" \
    -ngl "$NGL_GEMMA" \
    -fa "$FA_GEMMA" \
    -ctk "$KV_K_GEMMA" \
    -ctv "$KV_V_GEMMA" \
    -ub "$UBATCH" \
    -c "$CTX_GEMMA" \
    -t 4 \
    -np 1 \
    </dev/null >>"$logf" 2>&1 &
  disown 2>/dev/null || true
  echo $! >"$PID_DIR/llama-${PORT_GEMMA}.pid"
  log "Gemma 1070 port=$PORT_GEMMA ngl=$NGL_GEMMA ctx=$CTX_GEMMA pid=$(cat "$PID_DIR/llama-${PORT_GEMMA}.pid")"
}

probe() {
  curl -sf --max-time 5 "http://127.0.0.1:$1/v1/models" >/dev/null
}

cmd_start() {
  log "=== Qwen3.6 turbo MoE (2060+P106) + Gemma-4 (1070) ==="
  mkdir -p "$PID_DIR"
  stop_all_llm
  start_gemma
  sleep 3
  start_qwen
  log "waiting up to 300s (MoE fit + P106 split)…"
  local ok_q=0 ok_g=0
  for i in $(seq 1 60); do
    probe "$PORT_GEMMA" && ok_g=1
    probe "$PORT_QWEN" && ok_q=1
    [[ "$ok_q" == 1 && "$ok_g" == 1 ]] && break
    sleep 5
  done
  cmd_status
  [[ "$ok_q" == 1 && "$ok_g" == 1 ]] || {
    log "WARN: check logs $PID_DIR/*.log"
    tail -20 "$PID_DIR"/*.log 2>/dev/null || true
  }
}

cmd_stop() {
  stop_all_llm
  rm -f "$PID_DIR"/*.pid 2>/dev/null || true
  log "stopped"
}

cmd_status() {
  probe "$PORT_QWEN" && log "OK  qwen36 :$PORT_QWEN ($(basename "$MODEL_QWEN"))" || log "DOWN qwen36 :$PORT_QWEN"
  probe "$PORT_GEMMA" && log "OK  gemma4 :$PORT_GEMMA" || log "DOWN gemma4 :$PORT_GEMMA"
  log "p106_split=$USE_P106 fit_target=$FIT_TARGET_QWEN turbo=$(basename "$MODEL_QWEN")"
  nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv 2>/dev/null || true
  log "endpoints:"
  log "  qwen:  http://10.0.0.201:${PORT_QWEN}/v1/chat/completions"
  log "  gemma: http://10.0.0.201:${PORT_GEMMA}/v1/chat/completions"
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *)
    echo "Usage: $0 {start|stop|status}"
    exit 1
    ;;
esac
