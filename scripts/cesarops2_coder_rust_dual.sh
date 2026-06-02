#!/usr/bin/env bash
# cesarops2 dual LLM: Qwen2.5-Coder-14B on RTX 2060 (:5200), Rust-Coder-14B on GTX 1070 (:5571).
#
# Models (NFS):
#   /mnt/t440/models/qwen2.5-coder-14b-instruct-q4_k_m.gguf  (~8.4GB, fits 2060 with tuned ngl)
#   /mnt/t440/models/Fortytwo_Strand-Rust-Coder-14B-v1-Q6_K.gguf (~12GB file — partial GPU on 8GB 1070)
#
# Stops ZAYA :8010 if running (frees 2060 VRAM).
#
# Usage:
#   bash scripts/cesarops2_coder_rust_dual.sh start
#   bash scripts/cesarops2_coder_rust_dual.sh status
#   bash scripts/cesarops2_coder_rust_dual.sh stop
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2_research_lab.sh" ]] || REPO="/codebase/repos/wreckhunter2000-1"
MODELS="${MODELS:-/mnt/t440/models}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA=/home/cesarops/bin/llama-server
PID_DIR="${PID_DIR:-/tmp/cesarops2-coder-rust}"

MODEL_CODER="${MODEL_CODER:-$MODELS/qwen2.5-coder-14b-instruct-q4_k_m.gguf}"
MODEL_RUST="${MODEL_RUST:-$MODELS/Fortytwo_Strand-Rust-Coder-14B-v1-Q6_K.gguf}"
PORT_CODER="${PORT_CODER:-5200}"
PORT_RUST="${PORT_RUST:-5571}"
# llama.cpp device order on this box: CUDA0=2060, CUDA1=1070, CUDA2=P106
DEV_CODER="${DEV_CODER:-CUDA0}"
DEV_RUST="${DEV_RUST:-CUDA1}"
CTX_CODER="${CTX_CODER:-8192}"
CTX_RUST="${CTX_RUST:-4096}"
NGL_CODER="${NGL_CODER:-32}"
PARALLEL="${PARALLEL:-1}"
NGL_RUST="${NGL_RUST:-28}"

log() { echo "[coder-rust] $*"; }

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  pkill -f "llama-server.*-port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}

stop_zaya() {
  if pgrep -f 'serve_zaya_api.py' >/dev/null 2>&1; then
    log "stopping ZAYA API :8010 (frees 2060 VRAM)…"
    pkill -f 'serve_zaya_api.py' 2>/dev/null || true
    sleep 2
  fi
}

start_llm() {
  local model=$1 port=$2 dev=$3 ctx=$4 tag=$5 ngl=$6
  if [[ ! -f "$model" ]]; then
    log "ERROR: missing model: $model"
    return 1
  fi
  if [[ ! -x "$LLAMA" ]]; then
    log "ERROR: llama-server not found: $LLAMA"
    return 1
  fi
  stop_port "$port"
  mkdir -p "$PID_DIR"
  local logf="$PID_DIR/llama-${port}.log"
  setsid "$LLAMA" \
    -m "$model" \
    --host 0.0.0.0 \
    --port "$port" \
    -dev "$dev" \
    -ngl "$ngl" \
    -c "$ctx" \
    -t 4 \
    -np "$PARALLEL" \
    --timeout 600 \
    --reasoning off \
    </dev/null >>"$logf" 2>&1 &
  disown 2>/dev/null || true
  echo $! >"$PID_DIR/llama-${port}.pid"
  log "started $tag port=$port dev=$dev ngl=$ngl ctx=$ctx pid=$(cat "$PID_DIR/llama-${port}.pid") log=$logf"
}

probe() {
  local url=$1 name=$2
  if curl -sf --max-time 5 "$url" >/dev/null 2>&1; then
    log "  OK  $name — $url"
    return 0
  fi
  log "  --  $name — $url (not ready)"
  return 1
}

cmd_start() {
  log "=== Qwen Coder 14B (2060) + Rust-Coder 14B (1070) ==="
  stop_zaya
  stop_port "$PORT_CODER"
  stop_port "$PORT_RUST"
  sleep 1

  start_llm "$MODEL_CODER" "$PORT_CODER" "$DEV_CODER" "$CTX_CODER" "qwen-coder-2060" "$NGL_CODER"
  start_llm "$MODEL_RUST" "$PORT_RUST" "$DEV_RUST" "$CTX_RUST" "rust-coder-1070" "$NGL_RUST"

  log "waiting up to 120s for load…"
  local ok_c=0 ok_r=0
  for i in $(seq 1 24); do
    curl -sf --max-time 3 "http://127.0.0.1:${PORT_CODER}/v1/models" >/dev/null && ok_c=1
    curl -sf --max-time 3 "http://127.0.0.1:${PORT_RUST}/v1/models" >/dev/null && ok_r=1
    [[ "$ok_c" == 1 && "$ok_r" == 1 ]] && break
    sleep 5
  done
  cmd_status
  [[ "$ok_c" == 1 && "$ok_r" == 1 ]] || log "WARN: one or both endpoints not ready — check $PID_DIR/*.log"
}

cmd_stop() {
  log "=== stop ==="
  stop_zaya
  stop_port "$PORT_CODER"
  stop_port "$PORT_RUST"
  rm -f "$PID_DIR"/*.pid 2>/dev/null || true
  log "stopped"
}

cmd_status() {
  log "=== status ==="
  probe "http://127.0.0.1:${PORT_CODER}/v1/models" "qwen-coder-2060"
  probe "http://127.0.0.1:${PORT_RUST}/v1/models" "rust-coder-1070"
  nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv 2>/dev/null || true
  log "dispatch:"
  log "  coder: http://10.0.0.201:${PORT_CODER}/v1/chat/completions"
  log "  rust:  http://10.0.0.201:${PORT_RUST}/v1/chat/completions"
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
