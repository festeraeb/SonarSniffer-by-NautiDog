#!/usr/bin/env bash
# Start idle Cake fleet (cluster-key by default). Does not stop llama — caller must free GPUs.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

mkdir -p "$CAKE_PID_DIR"

if ! command -v "$CAKE" >/dev/null 2>&1 && [[ ! -x "$CAKE" ]]; then
  echo "[cake-fleet] Cake binary missing ($CAKE). Run: bash ${REPO}/scripts/install_cake_fleet.sh" | tee -a "$CAKE_LOG"
  exit 1
fi

MODE="${CAKE_FLEET_MODE:-cluster-key}"
echo "[cake-fleet] mode=$MODE model=$CAKE_MODEL" | tee -a "$CAKE_LOG"

stop_fleet_cluster() {
  bash "${SCRIPT_DIR}/stop-fleet-cluster.sh" || true
}

case "$MODE" in
  cluster-key)
    if [[ -z "$CAKE_CLUSTER_KEY" ]]; then
      echo "[cake-fleet] Set CAKE_CLUSTER_KEY or create $CAKE_CLUSTER_KEY_FILE" | tee -a "$CAKE_LOG"
      exit 1
    fi
    export CAKE_CLUSTER_KEY
    stop_fleet_cluster

    # Local worker (T440) — mDNS; layers assigned by master VRAM.
    setsid "$CAKE" run --cluster-key "$CAKE_CLUSTER_KEY" --name t440-fleet \
      >>"$CAKE_LOG" 2>&1 &
    echo $! >"${CAKE_PID_DIR}/worker-t440.pid"

    # Remote augment worker — prefer n8n/NFS dispatch (no SSH)
    if [[ "${FLEET_DISPATCH:-auto}" != "ssh" ]] && [[ -x "${REPO}/scripts/fleet-n8n-dispatch.sh" ]]; then
      bash "${REPO}/scripts/fleet-n8n-dispatch.sh" cesarops2 cake_worker_start \
        2>>"$CAKE_LOG" || true
    elif command -v ssh >/dev/null 2>&1; then
      ssh -o ConnectTimeout=5 "${CESAROPS2_USER}@${CESAROPS2_HOST}" \
        "bash '${REPO}/scripts/cake/start-worker-local.sh'" \
        2>>"$CAKE_LOG" | tee "${CAKE_PID_DIR}/worker-cesarops2.pid" || true
    fi

    setsid "$CAKE" serve "$CAKE_MODEL" --cluster-key "$CAKE_CLUSTER_KEY" \
      --api "$CAKE_SERVE_API" --discovery-timeout "$CAKE_DISCOVERY_TIMEOUT" \
      >>"$CAKE_LOG" 2>&1 &
    echo $! >"${CAKE_PID_DIR}/serve.pid"
    ;;

  topology)
    stop_fleet_cluster
    if [[ ! -f "$CAKE_TOPOLOGY" ]]; then
      echo "[cake-fleet] missing topology: $CAKE_TOPOLOGY" | tee -a "$CAKE_LOG"
      exit 1
    fi
    # Workers must be started per node before master (see docs/CAKE_INSTALL.md).
    setsid "$CAKE" serve "$CAKE_MODEL" --topology "$CAKE_TOPOLOGY" --api "$CAKE_SERVE_API" \
      >>"$CAKE_LOG" 2>&1 &
    echo $! >"${CAKE_PID_DIR}/serve.pid"
    ;;

  *)
    echo "[cake-fleet] unknown CAKE_FLEET_MODE=$MODE (cluster-key|topology)"
    exit 1
    ;;
esac

echo "[cake-fleet] started (PIDs in $CAKE_PID_DIR)" | tee -a "$CAKE_LOG"
