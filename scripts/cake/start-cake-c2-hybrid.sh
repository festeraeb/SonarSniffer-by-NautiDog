#!/usr/bin/env bash
# Cake on cesarops2: GPU VRAM + system RAM together.
#   - CPU/RAM worker :10132 (heavy layer bands)
#   - GPU workers :10129 (2060), :10130 (1070)
#   - Master --cpu (embeddings + orchestration in RAM, API :8081)
#
# MoE (fits c2):  USE_70B=0  → Qwen3.6-35B-A3B + --expert-offload
# 72B (needs RAM): USE_70B=1  → topology + ~52GB+ free RAM; set FREE_LLAMA=1
#
# Usage:
#   FREE_LLAMA=1 bash scripts/cake/start-cake-c2-hybrid.sh start
#   bash scripts/cake/start-cake-c2-hybrid.sh status
#   bash scripts/cake/start-cake-c2-hybrid.sh stop
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$SCRIPT_DIR/fleet-env.sh" ]] || REPO="/codebase/repos/wreckhunter2000-1"
[[ -f "$REPO/scripts/cesarops2-isolated.env" ]] && source "$REPO/scripts/cesarops2-isolated.env"
[[ -f "${HOME}/.cache/cesarops/cesarops2-isolated" ]] && source "$REPO/scripts/cesarops2-isolated.env" 2>/dev/null || true
export CAKE_C2_ONLY="${CAKE_C2_ONLY:-1}"

export USE_70B="${USE_70B:-0}"
export USE_HETERO_FLEET=0
export CAKE_FLEET_MODE=topology
export CAKE_SERVE_API="${CAKE_SERVE_API:-0.0.0.0:8081}"
export CAKE_TOPOLOGY="${CAKE_TOPOLOGY:-$SCRIPT_DIR/topology_fleet_c2_hybrid.yml}"
export CAKE_CLUSTER_KEY_FILE="${CAKE_CLUSTER_KEY_FILE:-$HOME/.cache/cesarops/cake-cluster.key}"

# shellcheck source=fleet-env.sh
source "$SCRIPT_DIR/fleet-env.sh"

CAKE_SM75="${CAKE_SM75:-/opt/cesarops/cake/bin/cake-sm75}"
CAKE_SM61="${CAKE_SM61:-/opt/cesarops/cake/bin/cake-sm61}"
[[ -x "$CAKE_SM75" ]] || CAKE_SM75="$CAKE"
[[ -x "$CAKE_SM61" ]] || CAKE_SM61="$CAKE"

FREE_LLAMA="${FREE_LLAMA:-0}"
WAIT_WORKERS="${WAIT_WORKERS:-180}"

log() { echo "[cake-c2-hybrid] $*" | tee -a "$CAKE_LOG"; }

stop_all() {
  bash "$SCRIPT_DIR/stop-fleet-cluster.sh" 2>/dev/null || true
  for port in 10129 10130 10132 8081; do
    fuser -k "${port}/tcp" 2>/dev/null || true
  done
  pkill -f 'cake run' 2>/dev/null || true
  pkill -f 'cake worker' 2>/dev/null || true
  pkill -f 'cake master' 2>/dev/null || true
  sleep 2
}

# Topology mode: legacy `worker` (no cluster-key — avoids UDP :10127 fights on one host).
start_topology_worker() {
  local bin=$1 dev=$2 port=$3 name=$4
  local extra=()
  [[ "$dev" == "cpu" ]] && extra=(--cpu) || extra=(--device "$dev")
  setsid env HF_HOME="$HF_HOME" "$bin" worker --model "$CAKE_MODEL" \
    --topology "$CAKE_TOPOLOGY" \
    "${extra[@]}" --name "$name" --address "0.0.0.0:${port}" \
    >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/worker-${name}.pid"
  log "c2 ${name} $(basename "$bin") dev=${dev} :${port} pid=$(cat "${CAKE_PID_DIR}/worker-${name}.pid")"
}

start_ram_worker() {
  setsid env HF_HOME="$HF_HOME" CUDA_VISIBLE_DEVICES="" "$CAKE" run --cluster-key "$CAKE_CLUSTER_KEY" \
    --cpu --name c2-ram --address "0.0.0.0:10132" \
    >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/worker-c2-ram.pid"
  log "c2-ram :10132 (cluster-key CPU) pid=$(cat "${CAKE_PID_DIR}/worker-c2-ram.pid")"
}

start_gpu_worker() {
  local dev=$1 port=$2 name=$3
  setsid env HF_HOME="$HF_HOME" "$CAKE" run --cluster-key "$CAKE_CLUSTER_KEY" \
    --device "$dev" --name "$name" --address "0.0.0.0:${port}" \
    >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/worker-${name}.pid"
  log "c2 ${name} cuda dev=${dev} :${port} pid=$(cat "${CAKE_PID_DIR}/worker-${name}.pid")"
}

start_moe_cluster_key() {
  # 35B MoE: cluster-key, master CPU + expert stream from disk/RAM
  export CAKE_FLEET_MODE=cluster-key
  stop_all
  start_ram_worker
  start_gpu_worker 1 10129 c2-rtx
  start_gpu_worker 2 10130 c2-1070
  sleep 5
  local extra=(--cpu --api "$CAKE_SERVE_API" --discovery-timeout 60 --expert-offload)
  setsid env CUDA_VISIBLE_DEVICES="" "$CAKE" run "$CAKE_MODEL" \
    --cluster-key "$CAKE_CLUSTER_KEY" \
    "${extra[@]}" \
    >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/serve.pid"
  log "master (CPU+expert-offload) model=$CAKE_MODEL pid=$(cat "${CAKE_PID_DIR}/serve.pid")"
}

start_topology_70b() {
  stop_all
  pkill -f 'cake worker' 2>/dev/null || true
  if [[ "$FREE_LLAMA" == "1" ]]; then
    log "FREE_LLAMA=1 — stopping llama on :5200/:5571 for GPU headroom"
    bash "$REPO/scripts/cesarops2_qwen36_gemma4_dual.sh" stop 2>/dev/null || true
    sleep 3
  fi
  export HF_HOME="${HF_HOME:-$HOME/.cache/huggingface}"
  export CAKE_MODEL="${CAKE_MODEL_70B:-Qwen/Qwen2.5-72B-Instruct}"
  # Stagger starts so discovery UDP :10127 is not contended.
  start_topology_worker "$CAKE_SM75" cpu 10132 c2-ram
  sleep 8
  start_topology_worker "$CAKE_SM75" 1 10129 c2-rtx
  sleep 8
  start_topology_worker "$CAKE_SM61" 2 10130 c2-1070
  log "workers loading shards — waiting ${WAIT_WORKERS}s before CPU master"
  sleep "$WAIT_WORKERS"
  setsid env HF_HOME="$HF_HOME" CUDA_VISIBLE_DEVICES="" "$CAKE_SM75" master --cpu \
    --model "$CAKE_MODEL" --topology "$CAKE_TOPOLOGY" --api "$CAKE_SERVE_API" \
    >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/serve.pid"
  log "master sm75 (CPU+topology) model=$CAKE_MODEL pid=$(cat "${CAKE_PID_DIR}/serve.pid")"
}

cmd_start() {
  if [[ "${USE_70B:-0}" != "1" ]]; then
    if [[ -z "${CAKE_CLUSTER_KEY:-}" ]]; then
      log "ERROR: set CAKE_CLUSTER_KEY or $CAKE_CLUSTER_KEY_FILE"
      exit 1
    fi
    export CAKE_CLUSTER_KEY
  fi
  export HF_HOME="${HF_HOME:-$HOME/.cache/huggingface}"
  mkdir -p "$CAKE_PID_DIR"
  if [[ "$USE_70B" == "0" ]]; then
    export CAKE_MODEL="${CAKE_MODEL:-$CAKE_MODEL_35B}"
    if [[ -n "${CAKE_MODEL_35B_LOCAL:-}" && -d "${CAKE_MODEL_35B_LOCAL}" ]]; then
      export CAKE_MODEL="$CAKE_MODEL_35B_LOCAL"
      log "using local MoE dir $CAKE_MODEL"
    elif ! cake list 2>/dev/null | grep -qE 'Qwen/Qwen3\.6-35B-A3B-Instruct.*complete'; then
      log "MoE NOT READY: no safetensors cache for $CAKE_MODEL_35B"
      log "  Hub returns 401 without HF_TOKEN — run: hf auth login && cake pull $CAKE_MODEL_35B"
      log "  Or use llama GGUF: bash scripts/cesarops2_qwen36_gemma4_dual.sh start"
      log "  Diagnose: bash scripts/cake/diagnose-moe-c2.sh"
      if [[ "${CAKE_FALLBACK_72B:-0}" == "1" ]]; then
        log "CAKE_FALLBACK_72B=1 — falling back to $CAKE_MODEL_70B"
        export USE_70B=1
        export CAKE_MODEL="$CAKE_MODEL_70B"
      else
        log "Refusing to start (set CAKE_FALLBACK_72B=1 to load 72B instead)"
        exit 1
      fi
    fi
  fi
  free -h | head -2 | tee -a "$CAKE_LOG"
  if [[ "$USE_70B" == "1" ]]; then
    log "mode=72B topology (GPU + system RAM)"
    start_topology_70b
  else
    log "mode=35B MoE cluster-key (GPU + RAM + expert-offload)"
    start_moe_cluster_key
  fi
  log "API target http://127.0.0.1:8081 — tail -f $CAKE_LOG"
}

cmd_status() {
  for port in 10129 10130 10132 8081; do
    timeout 1 bash -c "echo >/dev/tcp/127.0.0.1/${port}" 2>/dev/null \
      && echo "  OK   :${port}" || echo "  DOWN :${port}"
  done
  curl -sf --max-time 3 http://127.0.0.1:8081/v1/models >/dev/null \
    && echo "  OK   Cake inference API" || echo "  DOWN Cake inference API"
  tail -5 "$CAKE_LOG" 2>/dev/null || true
}

cmd_stop() {
  stop_all
  log "stopped"
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *)
    echo "Usage: $0 {start|stop|status}"
    echo "  USE_70B=0 (default) MoE on GPU+RAM | USE_70B=1 72B topology | FREE_LLAMA=1"
    exit 1
    ;;
esac
