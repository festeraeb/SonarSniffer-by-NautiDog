#!/usr/bin/env bash
# DeepSeek-R1 on cesarops2 via Cake (cluster-key + expert-offload + GPU/RAM workers).
# Requires full weights under ~/cesarops-data/models/DeepSeek-R1 (163 shards).
#
# Usage:
#   bash scripts/cake/start-cake-r1-c2.sh start
#   bash scripts/cake/start-cake-r1-c2.sh status
#   bash scripts/cake/start-cake-r1-c2.sh stop
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2-isolated.env" ]] && source "$REPO/scripts/cesarops2-isolated.env"
[[ -f "${HOME}/.cache/cesarops/cesarops2-isolated" ]] && source "$REPO/scripts/cesarops2-isolated.env" 2>/dev/null || true

# shellcheck source=fleet-env.sh
source "$SCRIPT_DIR/fleet-env.sh"

export CAKE_MODEL="${CAKE_MODEL:-deepseek-ai/DeepSeek-R1}"
export CAKE_MODEL_DIR="${CAKE_MODEL_DIR:-$HOME/cesarops-data/models/DeepSeek-R1}"
export HF_HOME="${HF_HOME:-$HOME/.cache/huggingface}"
export CAKE_CLUSTER_KEY_FILE="${CAKE_CLUSTER_KEY_FILE:-$HOME/.cache/cesarops/cake-cluster.key}"
export CAKE_SERVE_API="${CAKE_SERVE_API:-0.0.0.0:8081}"
TOTAL_SHARDS="${TOTAL_SHARDS:-163}"

CAKE_SM75="${CAKE_SM75:-/opt/cesarops/cake/bin/cake-sm75}"
CAKE_SM61="${CAKE_SM61:-/opt/cesarops/cake/bin/cake-sm61}"
[[ -x "$CAKE_SM75" ]] || CAKE_SM75="$CAKE"
[[ -x "$CAKE_SM61" ]] || CAKE_SM61="$CAKE"

log() { echo "[cake-r1] $*" | tee -a "$CAKE_LOG"; }

shard_count() {
  find "$CAKE_MODEL_DIR" -maxdepth 1 -name 'model-*.safetensors' 2>/dev/null | wc -l
}

require_model() {
  local n
  n=$(shard_count)
  if [[ ! -f "$CAKE_MODEL_DIR/config.json" ]]; then
    log "ERROR: missing $CAKE_MODEL_DIR/config.json"
    exit 1
  fi
  if [[ "$n" -lt "$TOTAL_SHARDS" ]]; then
    log "ERROR: incomplete weights $n/$TOTAL_SHARDS shards in $CAKE_MODEL_DIR"
    log "  wait for: bash scripts/download-deepseek-r1.sh"
    exit 1
  fi
  log "weights OK: $n/$TOTAL_SHARDS shards ($(du -sh "$CAKE_MODEL_DIR" | cut -f1))"
}

stop_all() {
  bash "$SCRIPT_DIR/start-cake-c2-hybrid.sh" stop 2>/dev/null || true
  bash "$SCRIPT_DIR/stop-fleet-cluster.sh" 2>/dev/null || true
  pkill -f 'cake run.*DeepSeek' 2>/dev/null || true
  pkill -f 'cake-sm.*DeepSeek' 2>/dev/null || true
  for port in 10129 10130 10132 8081; do
    fuser -k "${port}/tcp" 2>/dev/null || true
  done
  sleep 2
}

start_workers() {
  local key=$1
  setsid env HF_HOME="$HF_HOME" CUDA_VISIBLE_DEVICES="" "$CAKE" run --cluster-key "$key" \
    --cpu --name c2-ram --address "0.0.0.0:10132" >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/worker-c2-ram.pid"
  log "ram worker pid=$(cat "${CAKE_PID_DIR}/worker-c2-ram.pid")"
  sleep 5
  setsid env HF_HOME="$HF_HOME" "$CAKE" run --cluster-key "$key" \
    --device 1 --name c2-rtx --address "0.0.0.0:10129" >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/worker-c2-rtx.pid"
  setsid env HF_HOME="$HF_HOME" "$CAKE" run --cluster-key "$key" \
    --device 2 --name c2-1070 --address "0.0.0.0:10130" >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/worker-c2-1070.pid"
  log "gpu workers started — waiting 30s"
  sleep 30
}

cmd_start() {
  [[ -f "$CAKE_CLUSTER_KEY_FILE" ]] || { log "ERROR: missing $CAKE_CLUSTER_KEY_FILE"; exit 1; }
  export CAKE_CLUSTER_KEY="$(tr -d '[:space:]' <"$CAKE_CLUSTER_KEY_FILE")"
  mkdir -p "$CAKE_PID_DIR"
  require_model
  free -h | head -2 | tee -a "$CAKE_LOG"
  stop_all
  log "starting DeepSeek-R1 MoE cluster-key + expert-offload"
  log "model_dir=$CAKE_MODEL_DIR api=$CAKE_SERVE_API"
  start_workers "$CAKE_CLUSTER_KEY"
  # Prefer local dir; fallback to repo id (uses HF_HOME hub cache)
  local model_arg="$CAKE_MODEL_DIR"
  [[ -d "$model_arg" ]] || model_arg="$CAKE_MODEL"
  setsid env HF_HOME="$HF_HOME" CUDA_VISIBLE_DEVICES="" "$CAKE" run "$model_arg" \
    --cluster-key "$CAKE_CLUSTER_KEY" \
    --cpu --expert-offload \
    --api "$CAKE_SERVE_API" \
    --discovery-timeout 120 \
    >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/serve.pid"
  log "master pid=$(cat "${CAKE_PID_DIR}/serve.pid") — tail -f $CAKE_LOG"
  log "smoke: curl -s http://127.0.0.1:8081/v1/models"
}

cmd_status() {
  echo "shards: $(shard_count)/$TOTAL_SHARDS"
  for port in 10129 10130 10132 8081; do
    timeout 1 bash -c "echo >/dev/tcp/127.0.0.1/${port}" 2>/dev/null \
      && echo "  OK :$port" || echo "  DOWN :$port"
  done
  curl -sf --max-time 3 http://127.0.0.1:8081/v1/models >/dev/null \
    && echo "  OK Cake API" || echo "  DOWN Cake API"
  tail -5 "$CAKE_LOG" 2>/dev/null || true
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) stop_all; log "stopped" ;;
  status) cmd_status ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
