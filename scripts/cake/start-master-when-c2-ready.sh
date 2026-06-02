#!/usr/bin/env bash
# Start Cake master on T440 after all cesarops2 worker ports are listening.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

export USE_70B=1
export USE_HETERO_FLEET=1
export CAKE_DUAL_P100="${CAKE_DUAL_P100:-1}"
SKIP_P106="${SKIP_P106:-1}"
if [[ "$SKIP_P106" == "1" ]]; then
  if [[ "$CAKE_DUAL_P100" == "1" ]]; then
    export CAKE_TOPOLOGY="${SCRIPT_DIR}/topology_fleet_hetero_70b_dual_p100_no_p106.yml"
  else
    export CAKE_TOPOLOGY="${SCRIPT_DIR}/topology_fleet_hetero_70b_no_p106.yml"
  fi
else
  export CAKE_TOPOLOGY="${SCRIPT_DIR}/topology_fleet_hetero_70b.yml"
fi

CAKE="${CAKE:-/opt/cesarops/cake/bin/cake}"
CESAROPS2_HOST="${CESAROPS2_HOST:-10.0.0.201}"
if [[ "${SKIP_P106:-1}" == "1" ]]; then
  PORTS=(10129 10130)
else
  PORTS=(10128 10129 10130)
fi
if [[ "${CAKE_DUAL_P100:-1}" == "1" ]]; then
  T440_PORTS=(10131 10132 10133)
else
  T440_PORTS=(10131 10132)
fi
WAIT_SECS="${WAIT_SECS:-7200}"
POLL="${POLL:-15}"

log() { echo "[cake-master-wait] $*" | tee -a "$CAKE_LOG"; }

port_up() {
  local host="$1" port="$2"
  timeout 2 bash -c "echo >/dev/tcp/${host}/${port}" 2>/dev/null
}

log "waiting for c2 ${CESAROPS2_HOST} ports ${PORTS[*]} + T440 ${T440_PORTS[*]} (max ${WAIT_SECS}s)…"
deadline=$((SECONDS + WAIT_SECS))
while (( SECONDS < deadline )); do
  ok=1
  for p in "${PORTS[@]}"; do port_up "$CESAROPS2_HOST" "$p" || ok=0; done
  for p in "${T440_PORTS[@]}"; do port_up 127.0.0.1 "$p" || ok=0; done
  if [[ "$ok" == "1" ]]; then
    log "all worker ports up — starting master on :8081"
    pkill -f 'cake master' 2>/dev/null || true
    sleep 1
    setsid env CUDA_VISIBLE_DEVICES="" "$CAKE" master --cpu \
      --model "$CAKE_MODEL" --topology "$CAKE_TOPOLOGY" \
      --api "${CAKE_SERVE_API:-0.0.0.0:8081}" >>"$CAKE_LOG" 2>&1 &
    echo $! >"${CAKE_PID_DIR}/serve.pid"
    log "master pid=$(cat "${CAKE_PID_DIR}/serve.pid")"
  log "poll API: curl -s http://127.0.0.1:8081/v1/models"
    exit 0
  fi
  missing=""
  for p in "${PORTS[@]}"; do port_up "$CESAROPS2_HOST" "$p" || missing+=" c2:$p"; done
  for p in "${T440_PORTS[@]}"; do port_up 127.0.0.1 "$p" || missing+=" t440:$p"; done
  log "not ready (${missing# }) — recheck in ${POLL}s"
  sleep "$POLL"
done

log "timeout — c2 still downloading or workers crashed. tail logs on both hosts."
exit 1
