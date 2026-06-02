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

# Prefer cached HF id; override with CAKE_MODEL_35B for MoE beta.
CAKE_MODEL_35B="${CAKE_MODEL_35B:-Qwen/Qwen3.6-35B-A3B-Instruct}"
# Local-only fallback when Hub returns 401 (no token).
if [[ -z "${CAKE_MODEL_35B_LOCAL:-}" ]]; then
  for p in \
    "/data/cesarops/models/Qwen3.6-35B-A3B-Instruct" \
    "/mnt/t440/models/Qwen3.6-35B-A3B-Instruct" \
    "$HOME/cake-data/Qwen3.6-35B-A3B-Instruct"; do
    [[ -d "$p" ]] && CAKE_MODEL_35B_LOCAL="$p" && break
  done
fi

# Full dense / full MoE-base models for Cake (NOT distill checkpoints).
# Distill (do not use for "full" runs): DeepSeek-R1-Distill-Qwen-*, DeepSeek-R1-Distill-Llama-70B
CAKE_MODEL_70B="${CAKE_MODEL_70B:-Qwen/Qwen2.5-72B-Instruct}"          # 72B dense instruct (default)
CAKE_MODEL_QWEN3_FULL="${CAKE_MODEL_QWEN3_FULL:-Qwen/Qwen3-32B}"        # 33B dense Qwen3
CAKE_MODEL_QWEN_BASE="${CAKE_MODEL_QWEN_BASE:-Qwen/Qwen2.5-72B}"        # 72B dense base (no instruct)
# deepseek-ai/DeepSeek-V3 and DeepSeek-R1 are ~685B total — need datacenter GPUs, not this fleet.

USE_70B="${USE_70B:-0}"
USE_QWEN3_FULL="${USE_QWEN3_FULL:-0}"
if [[ "$USE_QWEN3_FULL" == "1" ]]; then
  CAKE_MODEL="$CAKE_MODEL_QWEN3_FULL"
elif [[ "$USE_70B" == "1" ]]; then
  CAKE_MODEL="$CAKE_MODEL_70B"
else
  CAKE_MODEL="$CAKE_MODEL_35B"
fi
if [[ -z "${CAKE_TOPOLOGY:-}" ]]; then
  if [[ "${USE_HETERO_FLEET:-1}" == "1" && "$USE_70B" == "1" ]]; then
    if [[ "${SKIP_P106:-1}" == "1" ]]; then
      CAKE_TOPOLOGY="${REPO}/scripts/cake/topology_fleet_hetero_70b_no_p106.yml"
    else
      CAKE_TOPOLOGY="${REPO}/scripts/cake/topology_fleet_hetero_70b.yml"
    fi
  elif [[ "$USE_QWEN3_FULL" == "1" ]]; then
    CAKE_TOPOLOGY="${REPO}/scripts/cake/topology_fleet_idle_70b_p1001.yml"
  elif [[ "$USE_70B" == "1" ]]; then
    CAKE_TOPOLOGY="${REPO}/scripts/cake/topology_fleet_idle_70b_p1001.yml"
  else
    CAKE_TOPOLOGY="${REPO}/scripts/cake/topology_fleet_idle_35b.yml"
  fi
fi

CAKE_CLUSTER_KEY_FILE="${CAKE_CLUSTER_KEY_FILE:-/etc/cesarops/cake-cluster.key}"
CAKE_CLUSTER_KEY="${CAKE_CLUSTER_KEY:-}"
if [[ -z "$CAKE_CLUSTER_KEY" && -f "$CAKE_CLUSTER_KEY_FILE" ]]; then
  CAKE_CLUSTER_KEY=$(tr -d '[:space:]' <"$CAKE_CLUSTER_KEY_FILE")
fi

CAKE_WORKER_PORT="${CAKE_WORKER_PORT:-10128}"
CAKE_SERVE_API="${CAKE_SERVE_API:-0.0.0.0:8081}"
CAKE_DISCOVERY_TIMEOUT="${CAKE_DISCOVERY_TIMEOUT:-30}"

# cesarops2 nvidia-smi: 0=P106, 1=2060 SUPER, 2=1070
# 70B hetero uses all three (start-fleet-hetero-70b.sh). 35B/cluster-key may use 2060 only.
CAKE_DEVICE="${CAKE_DEVICE:-1}"
CESAROPS2_HOST="${CESAROPS2_HOST:-10.0.0.201}"
CESAROPS2_USER="${CESAROPS2_USER:-cesarops}"
