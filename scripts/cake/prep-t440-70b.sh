#!/usr/bin/env bash
# T440 prep: dual P100 CUDA workers + RAM CPU worker (P106 layers on RAM). Does not touch c2.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

export USE_70B=1
export CAKE_DUAL_P100="${CAKE_DUAL_P100:-1}"
export CAKE_MODEL="${CAKE_MODEL_70B:-Qwen/Qwen2.5-72B-Instruct}"
export SKIP_P106="${SKIP_P106:-1}"
if [[ "$CAKE_DUAL_P100" == "1" ]]; then
  export CAKE_TOPOLOGY="${SCRIPT_DIR}/topology_fleet_hetero_70b_dual_p100_no_p106.yml"
fi

CAKE="${CAKE:-/opt/cesarops/cake/bin/cake}"
CAKE_SM60="${CAKE_SM60:-/opt/cesarops/cake/bin/cake-sm60}"
[[ -x "$CAKE_SM60" ]] || CAKE_SM60="$CAKE"

mkdir -p "$CAKE_PID_DIR"
log() { echo "[cake-prep-t440] $*" | tee -a "$CAKE_LOG"; }

log "T440 prep — dual P100 (sm60) + RAM; freeing llama :5001"
bash "${REPO}/scripts/p100_cycle.sh" free >>"$CAKE_LOG" 2>&1 || true

pkill -f 'cake master' 2>/dev/null || true
for pidf in "${CAKE_PID_DIR}/worker-t440"*.pid "${CAKE_PID_DIR}/serve.pid"; do
  [[ -f "$pidf" ]] || continue
  kill "$(cat "$pidf")" 2>/dev/null || true
done
pkill -f 'cake worker.*t440-' 2>/dev/null || true
sleep 2

cache="${HOME}/.cache/huggingface/hub/models--Qwen--Qwen2.5-72B-Instruct"
if [[ -d "$cache" ]]; then
  log "HF cache on T440: $(du -sh "$cache" 2>/dev/null | cut -f1)"
else
  log "WARN: no 72B HF cache on T440"
fi

if ! [[ -x "$CAKE" ]]; then
  log "ERROR: cake missing at $CAKE"
  exit 1
fi
if [[ "$CAKE_SM60" == "$CAKE" ]]; then
  log "WARN: cake-sm60 missing — run: bash scripts/cake/install_cake_for_cap.sh 60"
fi

if [[ "$CAKE_DUAL_P100" == "1" ]]; then
  setsid env CUDA_VISIBLE_DEVICES=0 "$CAKE_SM60" worker --model "$CAKE_MODEL" --device 0 \
    --name t440-p100-0 --address 0.0.0.0:10133 >>"$CAKE_LOG" 2>&1 &
  echo $! >"${CAKE_PID_DIR}/worker-t440-p100-0.pid"
  log "t440-p100-0 :10133 pid=$(cat "${CAKE_PID_DIR}/worker-t440-p100-0.pid")"
fi

setsid env CUDA_VISIBLE_DEVICES=1 "$CAKE_SM60" worker --model "$CAKE_MODEL" --device 0 \
  --name t440-p100-1 --address 0.0.0.0:10131 >>"$CAKE_LOG" 2>&1 &
echo $! >"${CAKE_PID_DIR}/worker-t440-p100-1.pid"
log "t440-p100-1 :10131 pid=$(cat "${CAKE_PID_DIR}/worker-t440-p100-1.pid")"

setsid env CUDA_VISIBLE_DEVICES="" "$CAKE" worker --model "$CAKE_MODEL" --cpu \
  --name t440-ram --address 0.0.0.0:10132 >>"$CAKE_LOG" 2>&1 &
echo $! >"${CAKE_PID_DIR}/worker-t440-ram.pid"
log "t440-ram :10132 pid=$(cat "${CAKE_PID_DIR}/worker-t440-ram.pid")"

log "T440 workers loading — when c2 :10129/:10130 up: bash ${SCRIPT_DIR}/start-master-when-c2-ready.sh"
log "Monitor: tail -f $CAKE_LOG"
