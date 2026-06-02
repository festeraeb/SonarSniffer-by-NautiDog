#!/usr/bin/env bash
# Heterogeneous Cake 72B: topology mode — dual T440 P100 + c2 2060/1070 + RAM (P106 out).
# Native-arch binaries: cake-sm60 (P100), cake-sm75 (2060), cake-sm61 (1070).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"

export USE_70B=1
export USE_HETERO_FLEET=1
export CAKE_MODEL="${CAKE_MODEL:-Qwen/Qwen2.5-72B-Instruct}"
export CAKE_FLEET_MODE=topology
export CAKE_SERVE_API="${CAKE_SERVE_API:-0.0.0.0:8081}"
export CAKE_DUAL_P100="${CAKE_DUAL_P100:-1}"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

SKIP_P106="${SKIP_P106:-1}"
if [[ "$SKIP_P106" == "1" ]]; then
  if [[ "$CAKE_DUAL_P100" == "1" ]]; then
    export CAKE_TOPOLOGY="${SCRIPT_DIR}/topology_fleet_hetero_70b_dual_p100_no_p106.yml"
    log_msg_p106="P106 out; dual P100 + RAM 0-9/70-79"
  else
    export CAKE_TOPOLOGY="${SCRIPT_DIR}/topology_fleet_hetero_70b_no_p106.yml"
    log_msg_p106="P106 skipped — layers 0-9 on T440 RAM"
  fi
else
  export CAKE_TOPOLOGY="${SCRIPT_DIR}/topology_fleet_hetero_70b.yml"
  log_msg_p106="P106 enabled on :10128"
fi

CAKE="${CAKE:-/opt/cesarops/cake/bin/cake}"
CAKE_SM60="${CAKE_SM60:-/opt/cesarops/cake/bin/cake-sm60}"
CAKE_SM75="${CAKE_SM75:-/opt/cesarops/cake/bin/cake-sm75}"
CAKE_SM61="${CAKE_SM61:-/opt/cesarops/cake/bin/cake-sm61}"
# P100 (sm_60) cannot compile cake WMMA kernels — use CPU workers on P100 ports.
P100_USE_CPU="${P100_USE_CPU:-1}"
[[ -x "$CAKE_SM60" ]] && strings "$CAKE_SM60" 2>/dev/null | grep -q 'sm_60' && P100_USE_CPU=0
[[ -x "$CAKE_SM75" ]] || CAKE_SM75="$CAKE"
[[ -x "$CAKE_SM61" ]] || CAKE_SM61="$CAKE"

CESAROPS2_HOST="${CESAROPS2_HOST:-10.0.0.201}"
CESAROPS2_USER="${CESAROPS2_USER:-cesarops}"
FREE_LLAMA="${FREE_LLAMA:-1}"

mkdir -p "$CAKE_PID_DIR"
log() { echo "[cake-hetero] $*" | tee -a "$CAKE_LOG"; }

if ! [[ -x "$CAKE" ]]; then
  log "cake missing at $CAKE — run: bash scripts/cake/install_cake_pascal_fleet.sh"
  exit 1
fi

bash "${SCRIPT_DIR}/stop-fleet-cluster.sh" 2>/dev/null || true
if [[ "${FREE_LLAMA:-0}" == "1" ]]; then
  log "freeing P100#0 — stopping llama :5001/:5002 for Cake"
  bash "${REPO}/scripts/p100_cycle.sh" free >>"$CAKE_LOG" 2>&1 || true
fi
pkill -f 'cake master' 2>/dev/null || true
sleep 2

log "topology=$CAKE_TOPOLOGY model=$CAKE_MODEL (${log_msg_p106:-})"
log "binaries: sm60=$CAKE_SM60 sm75=$CAKE_SM75 sm61=$CAKE_SM61"

# --- cesarops2: 2060 + 1070 (no P106) ---
if [[ "${PREP_T440_ONLY:-0}" == "1" ]]; then
  log "PREP_T440_ONLY=1 — skipping c2 SSH"
elif command -v ssh >/dev/null 2>&1; then
  export SKIP_P106 CAKE_SM75 CAKE_SM61 CAKE_MODEL
  ssh -o ConnectTimeout=10 "${CESAROPS2_USER}@${CESAROPS2_HOST}" bash -s <<REMOTE
set -euo pipefail
CAKE_SM75="${CAKE_SM75:-/opt/cesarops/cake/bin/cake-sm75}"
CAKE_SM61="${CAKE_SM61:-/opt/cesarops/cake/bin/cake-sm61}"
[[ -x "\$CAKE_SM75" ]] || CAKE_SM75=/opt/cesarops/cake/bin/cake
[[ -x "\$CAKE_SM61" ]] || CAKE_SM61=/opt/cesarops/cake/bin/cake
LOG=~/.cache/cesarops/cake_fleet.log
PIDDIR=~/.cache/cesarops/cake-fleet-pids
mkdir -p "\$PIDDIR"
pkill -f 'cake worker' 2>/dev/null || true
sleep 2
start_w() {
  local bin=\$1 dev=\$2 port=\$3 name=\$4
  setsid "\$bin" worker --model "${CAKE_MODEL}" --device "\$dev" --name "\$name" --address "0.0.0.0:\${port}" >>"\$LOG" 2>&1 &
  echo \$! >"\$PIDDIR/worker-\${name}.pid"
  echo "[cake-hetero] c2 \${name} bin=\$(basename \$bin) gpu=\${dev} :\${port} pid=\$(cat \$PIDDIR/worker-\${name}.pid)"
}
start_w "\$CAKE_SM75" 1 10129 c2-rtx
start_w "\$CAKE_SM61" 2 10130 c2-1070
REMOTE
else
  log "WARN: no ssh — start c2 workers manually (:10129 sm75, :10130 sm61)"
fi

# --- T440: P100 layer workers (CPU fallback on Pascal) + RAM ---
start_p100_worker() {
  local name=$1 port=$2 gpu=$3
  if [[ "$P100_USE_CPU" == "1" ]]; then
    setsid env CUDA_VISIBLE_DEVICES="" "$CAKE" worker --model "$CAKE_MODEL" --cpu \
      --name "$name" --address "0.0.0.0:${port}" >>"$CAKE_LOG" 2>&1 &
  else
    setsid env CUDA_VISIBLE_DEVICES="$gpu" "$CAKE_SM60" worker --model "$CAKE_MODEL" --device 0 \
      --name "$name" --address "0.0.0.0:${port}" >>"$CAKE_LOG" 2>&1 &
  fi
  echo $! >"${CAKE_PID_DIR}/worker-${name}.pid"
  log "T440 ${name} :${port} ($([[ "$P100_USE_CPU" == 1 ]] && echo CPU || echo CUDA)) pid=$(cat "${CAKE_PID_DIR}/worker-${name}.pid")"
}
if [[ "$CAKE_DUAL_P100" == "1" ]]; then
  start_p100_worker t440-p100-0 10133 0
fi
start_p100_worker t440-p100-1 10131 1

setsid env CUDA_VISIBLE_DEVICES="" "$CAKE" worker --model "$CAKE_MODEL" --cpu \
  --name t440-ram --address 0.0.0.0:10132 >>"$CAKE_LOG" 2>&1 &
echo $! >"${CAKE_PID_DIR}/worker-t440-ram.pid"
log "T440 RAM :10132 pid=$(cat "${CAKE_PID_DIR}/worker-t440-ram.pid")"

log "workers loading — run: bash ${SCRIPT_DIR}/start-master-when-c2-ready.sh"
log "or wait ${WAIT_WORKERS:-0}s and start master inline (WAIT_WORKERS>0)"
if [[ "${WAIT_WORKERS:-0}" -gt 0 ]]; then
  sleep "$WAIT_WORKERS"
  setsid env CUDA_VISIBLE_DEVICES="" "$CAKE" master --cpu \
    --model "$CAKE_MODEL" --topology "$CAKE_TOPOLOGY" \
    --api "$CAKE_SERVE_API" >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/serve.pid"
  log "master pid=$(cat "${CAKE_PID_DIR}/serve.pid") — API http://127.0.0.1:8081"
fi
log "tail -f $CAKE_LOG"
