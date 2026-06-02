#!/usr/bin/env bash
# P106 (:5201) — PAMP bootstrap / thinker_fast lane.
# Fast expert-routing model (NOT DeepSeek-R1-7B — poor on design/classify tests).
#
# Default: Gemma-4-E4B Q4 (best PAMP JSON; ~4.7G file)
# Alt:      MODEL_PAMP=$MODELS/Phi-3-mini-4k-instruct-Q4_K_M.gguf
# Alt:      MODEL_PAMP=$MODELS/qwen2.5-coder-1.5b-instruct-q6_k.gguf
#
# Forge: [roles] bootstrap / thinker_fast → http://10.0.0.201:5201
#         preset pamp-moe-predict + golden suite PAMP
#
#   bash scripts/cesarops2_pamp_p106.sh start
#   bash scripts/cesarops2_pamp_p106.sh smoke
#   bash scripts/cesarops2_pamp_p106.sh stop
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2_pamp_p106.sh" ]] || REPO="/data/codebase/repos/wreckhunter2000-1"
MODELS="${MODELS:-/mnt/t440/models}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
PID_DIR="${PID_DIR:-/tmp/cesarops2-pamp-p106}"
LOG="${LOG:-/data/cesarops/logs/cesarops2-pamp-p106.log}"

PORT="${PORT:-5201}"
# llama CUDA2 = P106 on cesarops2
DEV="${DEV:-CUDA2}"
CTX="${CTX:-4096}"
NGL="${NGL:-99}"

MODEL_PAMP="${MODEL_PAMP:-$MODELS/gemma-4-E4B-it-Q4_K_M.gguf}"

log() { echo "[pamp-p106] $*" | tee -a "$LOG"; }

stop_port() {
  pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
}

cmd_start() {
  [[ -f "$MODEL_PAMP" ]] || { log "missing model: $MODEL_PAMP"; exit 1; }
  mkdir -p "$PID_DIR" "$(dirname "$LOG")"
  stop_port
  sleep 2
  log "loading $(basename "$MODEL_PAMP") on :$PORT dev=$DEV (PAMP bootstrap)"
  setsid "$LLAMA" -m "$MODEL_PAMP" --host 0.0.0.0 --port "$PORT" -dev "$DEV" \
    -ngl "$NGL" -fa auto -ctk f16 -ctv f16 -ub 384 -c "$CTX" -t 4 -np 1 \
    --reasoning off --timeout 300 \
    </dev/null >>"$PID_DIR/llama-${PORT}.log" 2>&1 &
  echo $! >"$PID_DIR/llama-${PORT}.pid"
  for i in $(seq 1 36); do
    if curl -sf --max-time 5 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
      curl -sf "http://127.0.0.1:${PORT}/v1/models" | head -c 200
      echo
      nvidia-smi --query-gpu=index,memory.used --format=csv,noheader
      log "READY http://10.0.0.201:${PORT}/v1/chat/completions"
      return 0
    fi
    sleep 5
  done
  tail -15 "$PID_DIR/llama-${PORT}.log" 2>/dev/null || true
  exit 1
}

cmd_smoke() {
  local prompt='Golden PAMP MoE predictor test: classify this as architecture debug task. Reply JSON only: {"parallel":["expert_a"],"serial_after":["expert_b"],"task_type":"architecture|execute|fast"}'
  curl -sf "http://127.0.0.1:${PORT}/v1/chat/completions" \
    -H 'Content-Type: application/json' \
    -d "$(python3 -c "
import json
print(json.dumps({
  'model': 'default',
  'messages': [
    {'role': 'system', 'content': 'You are the PAMP bootstrap router. Short JSON only.'},
    {'role': 'user', 'content': '''$prompt'''},
  ],
  'temperature': 0.1,
  'max_tokens': 256,
}))
")" | python3 -c "
import json,sys
d=json.load(sys.stdin)
t=d['choices'][0]['message'].get('content','')
print(t[:1200])
print('--- words', len(t.split()))
"
}

cmd_stop() { stop_port; log "stopped :$PORT"; }
cmd_status() {
  curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" && echo || log "DOWN :$PORT"
}

case "${1:-start}" in
  start) cmd_start ;;
  smoke) cmd_smoke ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *) echo "Usage: $0 {start|smoke|stop|status}"; exit 1 ;;
esac
