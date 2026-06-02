# shellcheck shell=bash
# Shared fleet Cake settings (source from other scripts).
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
STATE_DIR="${STATE_DIR:-/home/cesarops/.cache/cesarops}"
CAKE_PID_DIR="${STATE_DIR}/cake-fleet-pids"
CAKE_LOG="${STATE_DIR}/cake_fleet.log"

# Prefer crates.io / cargo install, then /opt symlink, then legacy build path.
if [[ -z "${CAKE:-}" ]]; then
  if [[ -x /opt/cesarops/cake/bin/cake ]]; then
    CAKE=/opt/cesarops/cake/bin/cake
  elif [[ -x "${HOME}/.cargo/bin/cake" ]]; then
    CAKE="${HOME}/.cargo/bin/cake"
  elif [[ -x /opt/cesarops/cake/target/release/cake ]]; then
    CAKE=/opt/cesarops/cake/target/release/cake
  elif [[ -x "${HOME}/benchmark/cake/target/release/cake" ]]; then
    CAKE="${HOME}/benchmark/cake/target/release/cake"
  else
    CAKE=cake
  fi
fi

CAKE_MODEL_35B="${CAKE_MODEL_35B:-Qwen/Qwen3.6-35B-A3B-Instruct}"
CAKE_MODEL_70B="${CAKE_MODEL_70B:-Qwen/Qwen2.5-72B-Instruct}"
USE_70B="${USE_70B:-0}"
if [[ "$USE_70B" == "1" ]]; then
  CAKE_MODEL="$CAKE_MODEL_70B"
  CAKE_TOPOLOGY="${REPO}/scripts/cake/topology_fleet_idle_70b.yml"
else
  CAKE_MODEL="$CAKE_MODEL_35B"
  CAKE_TOPOLOGY="${REPO}/scripts/cake/topology_fleet_idle_35b.yml"
fi

CAKE_CLUSTER_KEY_FILE="${CAKE_CLUSTER_KEY_FILE:-/etc/cesarops/cake-cluster.key}"
CAKE_CLUSTER_KEY="${CAKE_CLUSTER_KEY:-}"
if [[ -z "$CAKE_CLUSTER_KEY" && -f "$CAKE_CLUSTER_KEY_FILE" ]]; then
  CAKE_CLUSTER_KEY=$(tr -d '[:space:]' <"$CAKE_CLUSTER_KEY_FILE")
fi

CAKE_WORKER_PORT="${CAKE_WORKER_PORT:-10128}"
CAKE_SERVE_API="${CAKE_SERVE_API:-0.0.0.0:8081}"
CAKE_DISCOVERY_TIMEOUT="${CAKE_DISCOVERY_TIMEOUT:-30}"

# Augment node (RTX 2060 only — never intake :5599 / P106 / 1070)
CESAROPS2_HOST="${CESAROPS2_HOST:-10.0.0.201}"
CESAROPS2_USER="${CESAROPS2_USER:-cesarops}"
