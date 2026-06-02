#!/usr/bin/env bash
# Pull Cake fleet models (35B idle + two 70B-class instruct models). Large downloads.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
# shellcheck source=scripts/cake/fleet-env.sh
source "${REPO}/scripts/cake/fleet-env.sh"

SKIP_35B="${SKIP_35B:-0}"
SKIP_70B="${SKIP_70B:-0}"

# Best open 70B+ instruct targets for Cake fleet (Phase 6 topology).
# Override: CAKE_PULL_70B_MODELS="Qwen/..." bash scripts/cake_pull_fleet_models.sh
# Full dense instruct models only (no R1-Distill-* / Distill-Llama-70B).
CAKE_PULL_70B_MODELS="${CAKE_PULL_70B_MODELS:-Qwen/Qwen2.5-72B-Instruct Qwen/Qwen2.5-72B}"
CAKE_PULL_QWEN3_FULL="${CAKE_PULL_QWEN3_FULL:-Qwen/Qwen3-32B}"

log() { echo "[cake_pull] $*"; }

if ! command -v "$CAKE" >/dev/null 2>&1 && [[ ! -x "$CAKE" ]]; then
  log "Cake CLI missing — run: bash ${REPO}/scripts/install_cake_fleet.sh"
  exit 1
fi

pull_one() {
  local id="$1"
  log "Pulling $id (this may take a long time)…"
  if "$CAKE" download --help >/dev/null 2>&1; then
    "$CAKE" download "$id"
  else
    "$CAKE" pull "$id"
  fi
}

if [[ "$SKIP_35B" != "1" ]]; then
  pull_one "$CAKE_MODEL_35B"
fi

if [[ "$SKIP_70B" != "1" ]]; then
  for m in $CAKE_PULL_70B_MODELS; do
    pull_one "$m"
  done
fi

if [[ "${PULL_QWEN3_FULL:-0}" == "1" ]]; then
  pull_one "$CAKE_PULL_QWEN3_FULL"
fi

log "Done. USE_70B=1 → ${CAKE_MODEL_70B} | USE_QWEN3_FULL=1 → ${CAKE_MODEL_QWEN3_FULL}"
