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

# On cesarops2: never start T440 workers or SSH to T440 (Zaya on P100).
CESAROPS2_ISOLATED="${CESAROPS2_ISOLATED:-0}"
[[ -f "${HOME}/.cache/cesarops/cesarops2-isolated" ]] && CESAROPS2_ISOLATED=1
[[ "${CAKE_C2_ONLY:-0}" == "1" ]] && CESAROPS2_ISOLATED=1

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

    if [[ "$CESAROPS2_ISOLATED" == "1" ]]; then
      echo "[cake-fleet] CESAROPS2_ISOLATED=1 — use: bash scripts/cake/start-cake-c2-hybrid.sh" | tee -a "$CAKE_LOG"
      echo "[cake-fleet] skipping T440 worker + remote dispatch (T440 untouched)" | tee -a "$CAKE_LOG"
      exit 0
    fi

    # Local worker (T440) — omit model; cluster-key enables worker mode.
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

    # Master: provide model as positional arg and enable API.
    setsid "$CAKE" run "$CAKE_MODEL" --cluster-key "$CAKE_CLUSTER_KEY" \
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
    setsid "$CAKE" master --model "$CAKE_MODEL" --topology "$CAKE_TOPOLOGY" --api "$CAKE_SERVE_API" \
      >>"$CAKE_LOG" 2>&1 &
    echo $! >"${CAKE_PID_DIR}/serve.pid"
    ;;

  *)
    echo "[cake-fleet] unknown CAKE_FLEET_MODE=$MODE (cluster-key|topology)"
    exit 1
    ;;
esac

echo "[cake-fleet] started (PIDs in $CAKE_PID_DIR)" | tee -a "$CAKE_LOG"
