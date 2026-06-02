#!/usr/bin/env bash
# Start Cake cluster worker on *this* host only (no SSH).
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

if ! command -v "$CAKE" >/dev/null 2>&1 && [[ ! -x "$CAKE" ]]; then
  echo "[cake-worker] missing binary: $CAKE" >&2
  exit 1
fi
if [[ -z "${CAKE_CLUSTER_KEY:-}" ]]; then
  echo "[cake-worker] CAKE_CLUSTER_KEY or $CAKE_CLUSTER_KEY_FILE required" >&2
  exit 1
fi

export CAKE_CLUSTER_KEY
NAME="${CAKE_WORKER_NAME:-$(hostname -s)-fleet}"
# nvidia-smi GPU 1 = RTX 2060 on cesarops2 (P106=0, 1070=2)
DEVICE="${CAKE_DEVICE:-1}"
setsid "$CAKE" run --cluster-key "$CAKE_CLUSTER_KEY" --name "$NAME" --device "$DEVICE" \
  >>"${CAKE_LOG}" 2>&1 &
echo $! >"${CAKE_PID_DIR}/worker-local.pid"
echo "[cake-worker] started $NAME pid=$!"
