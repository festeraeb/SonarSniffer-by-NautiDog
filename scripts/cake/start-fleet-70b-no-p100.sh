#!/usr/bin/env bash
# Cake 70B: T440 RAM + P100#1 (16GB CUDA) + cesarops2 RTX. P100#0 reserved for Gemma :5001 only.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

export USE_70B=1
export CAKE_MODEL="${CAKE_MODEL_70B:-Qwen/Qwen2.5-72B-Instruct}"
export CAKE_FLEET_MODE="${CAKE_FLEET_MODE:-cluster-key}"
export CAKE_SERVE_API="${CAKE_SERVE_API:-0.0.0.0:8081}"
export CAKE_DISCOVERY_TIMEOUT="${CAKE_DISCOVERY_TIMEOUT:-60}"

CAKE="${CAKE:-/opt/cesarops/cake/bin/cake}"
[[ -x "$CAKE" ]] || CAKE="${HOME}/.cargo/bin/cake"

KEYFILE="/home/cesarops/.cache/cesarops/cake-cluster.key"
if [[ ! -f "$KEYFILE" ]]; then
  mkdir -p "$(dirname "$KEYFILE")"
  openssl rand -hex 24 >"$KEYFILE"
  chmod 600 "$KEYFILE"
fi
export CAKE_CLUSTER_KEY="$(tr -d '[:space:]' <"$KEYFILE")"
export CAKE_CLUSTER_KEY_FILE="$KEYFILE"

mkdir -p "$CAKE_PID_DIR"
log() { echo "[cake-70b] $*" | tee -a "$CAKE_LOG"; }

if ! [[ -x "$CAKE" ]]; then
  log "cake missing — copy from cesarops2: scp cesarops@10.0.0.201:/opt/cesarops/cake/bin/cake $CAKE"
  exit 1
fi

bash "${SCRIPT_DIR}/stop-fleet-cluster.sh" 2>/dev/null || true

log "model=$CAKE_MODEL mode=$CAKE_FLEET_MODE api=$CAKE_SERVE_API"

# T440: P100 #1 only (CUDA1 / 16GB). One worker per host (UDP :10127); master --cpu covers T440 RAM.
setsid env CUDA_VISIBLE_DEVICES=1 "$CAKE" worker --device 0 \
  --cluster-key "$CAKE_CLUSTER_KEY" --name t440-p100-1 \
  --address 0.0.0.0:10131 >>"$CAKE_LOG" 2>&1 &
echo $! >"${CAKE_PID_DIR}/worker-t440-p100-1.pid"
log "T440 P100#1 VRAM worker :10131 pid=$(cat "${CAKE_PID_DIR}/worker-t440-p100-1.pid")"

# cesarops2: RTX 2060 (GPU 1) + optional CPU worker
if command -v ssh >/dev/null 2>&1; then
  ssh -o ConnectTimeout=8 "${CESAROPS2_USER}@${CESAROPS2_HOST}" bash -s <<REMOTE
set -euo pipefail
export CAKE_CLUSTER_KEY='$CAKE_CLUSTER_KEY'
CAKE=/opt/cesarops/cake/bin/cake
mkdir -p ~/.cache/cesarops/cake-fleet-pids
pkill -f 'cake worker.*cluster-key' 2>/dev/null || true
sleep 1
# One worker per host (cluster discovery uses UDP :10127). RTX 2060 = GPU 1.
setsid \$CAKE worker --cluster-key "\$CAKE_CLUSTER_KEY" --name c2-rtx --device 1 \
  --address 0.0.0.0:10128 >>~/.cache/cesarops/cake_fleet.log 2>&1 &
echo \$! >~/.cache/cesarops/cake-fleet-pids/worker-rtx.pid
echo "[cake-70b] cesarops2 RTX worker :10128"
REMOTE
fi

sleep 5

# Master on T440 CPU (coordinates cluster, serves :8081)
setsid env CUDA_VISIBLE_DEVICES="" "$CAKE" master --cpu \
  --model "$CAKE_MODEL" --cluster-key "$CAKE_CLUSTER_KEY" \
  --api "$CAKE_SERVE_API" --discovery-timeout "$CAKE_DISCOVERY_TIMEOUT" \
  >>"$CAKE_LOG" 2>&1 &
echo $! >"${CAKE_PID_DIR}/serve.pid"
log "master pid=$(cat "${CAKE_PID_DIR}/serve.pid") — API http://127.0.0.1:8081 (after model load)"

log "Pull model if needed: $CAKE download $CAKE_MODEL"
log "Smoke: curl -s http://127.0.0.1:8081/v1/models || tail -f $CAKE_LOG"
