#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

kill_pidfile() {
  local f="$1"
  [[ -f "$f" ]] || return 0
  local pid
  pid=$(cat "$f")
  kill "$pid" 2>/dev/null || true
  rm -f "$f"
}

for f in "${CAKE_PID_DIR:-$STATE_DIR/cake-fleet-pids}"/*.pid; do
  [[ -e "$f" ]] || continue
  kill_pidfile "$f"
done

pkill -f 'cake (run|serve).*cluster-key' 2>/dev/null || true
pkill -f 'cake (run|serve).*--topology' 2>/dev/null || true

if [[ "${FLEET_DISPATCH:-auto}" != "ssh" ]] && [[ -x "${REPO}/scripts/fleet-n8n-dispatch.sh" ]]; then
  bash "${REPO}/scripts/fleet-n8n-dispatch.sh" cesarops2 cake_worker_stop 2>/dev/null || true
elif command -v ssh >/dev/null 2>&1; then
  ssh -o ConnectTimeout=5 "${CESAROPS2_USER:-cesarops}@${CESAROPS2_HOST:-10.0.0.201}" \
    "pkill -f 'cake run.*cluster-key' 2>/dev/null || true" 2>/dev/null || true
fi

echo "[cake-fleet] stopped" >>"${CAKE_LOG:-$STATE_DIR/cake_fleet.log}" 2>/dev/null || true
