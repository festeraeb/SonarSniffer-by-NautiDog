#!/usr/bin/env bash
# Fleet resilience tick for n8n + Forge across T440 and cesarops2.
# - Tries local restart for n8n and Forge when unhealthy.
# - Probes peer node and enqueues recovery handoff jobs when peer is unhealthy.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
if [[ -d /mnt/t440/codebase/repos/wreckhunter2000-1 ]]; then
  REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"
elif [[ -d /mnt/t440/repo ]]; then
  REPO="/mnt/t440/repo"
fi

N8N_PORT="${N8N_PORT:-5678}"
FORGE_PORT="${FORGE_PORT:-9100}"
LOG="${N8N_WATCHDOG_LOG:-/home/cesarops/n8n-watchdog.log}"
MAX_TRIES="${N8N_WATCHDOG_MAX_TRIES:-2}"

# Scan-safe guard: when present, watchdog exits immediately (no health probes,
# no restarts, avoids model-loading side effects while scanning).
WATCHDOG_GUARD="${CESAROPS_SCAN_NO_WATCHDOG_GUARD:-/tmp/cesarops-scan-no-watchdog}"
LLM_WATCHDOG_GUARD="${FORGE_LLM_WATCHDOG_GUARD:-/tmp/cesarops-llm-watchdog-off}"
if [[ -f "$WATCHDOG_GUARD" ]]; then
  echo "[$(date -Iseconds)] scan-safe guard present ($WATCHDOG_GUARD); exiting n8n-watchdog" >>"$LOG" 2>/dev/null || true
  exit 0
fi
if [[ -f "$LLM_WATCHDOG_GUARD" ]]; then
  echo "[$(date -Iseconds)] llm-watchdog-off guard ($LLM_WATCHDOG_GUARD); exiting n8n-watchdog" >>"$LOG" 2>/dev/null || true
  exit 0
fi

hn="$(hostname -s | tr '[:upper:]' '[:lower:]')"
hn="${hn%%.*}"
if [[ "$hn" == *t440* ]]; then
  NODE="t440"
  PEER_NODE="cesarops2"
  PEER_N8N_URL="${PEER_N8N_URL:-http://10.0.0.201:5678}"
  PEER_FORGE_URL="${PEER_FORGE_URL:-http://10.0.0.201:${FORGE_PORT}}"
else
  NODE="cesarops2"
  PEER_NODE="t440"
  PEER_N8N_URL="${PEER_N8N_URL:-http://10.0.0.61:5678}"
  PEER_FORGE_URL="${PEER_FORGE_URL:-http://10.0.0.61:${FORGE_PORT}}"
fi

N8N_URL="${N8N_URL:-http://127.0.0.1:${N8N_PORT}}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:${FORGE_PORT}}"

ts() { date -Iseconds; }
log() { echo "$(ts) $*" >>"$LOG"; }

probe() {
  local url="$1"
  curl -sf --max-time 3 "$url" >/dev/null
}

dispatch_handoff() {
  local node="$1"
  local action="$2"
  local reason="$3"
  if [[ ! -x "${REPO}/scripts/fleet-n8n-dispatch.sh" ]]; then
    log "handoff skipped: fleet-n8n-dispatch.sh missing"
    return 0
  fi
  bash "${REPO}/scripts/fleet-n8n-dispatch.sh" "$node" "$action" \
    origin_node="$NODE" trigger="n8n-watchdog" reason="$reason" \
    >>"$LOG" 2>&1 || true
}

ensure_watchdog_guard() {
  if [[ ! -f "${REPO}/scripts/ensure_n8n_watchdog.sh" ]]; then
    log "watchdog guard helper missing: scripts/ensure_n8n_watchdog.sh"
    return 1
  fi
  if ! bash "${REPO}/scripts/ensure_n8n_watchdog.sh" >>"$LOG" 2>&1; then
    log "watchdog guard failed on ${NODE}; enqueue local repair"
    dispatch_handoff "$NODE" "ensure_n8n_watchdog" "watchdog_guard_failed"
    return 1
  fi
  return 0
}

ensure_local_n8n() {
  if probe "${N8N_URL}/healthz"; then
    return 0
  fi
  local attempt
  for attempt in $(seq 1 "$MAX_TRIES"); do
    log "n8n unhealthy on ${NODE} attempt=${attempt}; restarting"
    bash "${REPO}/scripts/start_n8n.sh" >>"$LOG" 2>&1 || true
    if probe "${N8N_URL}/healthz"; then
      log "n8n recovered on ${NODE}"
      return 0
    fi
  done
  log "n8n still unhealthy on ${NODE}; escalating"
  dispatch_handoff "$NODE" "restart_n8n" "local_n8n_unhealthy"
  dispatch_handoff "$NODE" "mission_service_watchdog" "local_n8n_llm_handoff"
  return 1
}

ensure_local_forge() {
  # T440: no local Forge — only verify peer (cesarops2) is up.
  if [[ "$NODE" == "t440" ]]; then
    if probe "${PEER_FORGE_URL}/health"; then
      return 0
    fi
    log "T440: peer Forge down at ${PEER_FORGE_URL}; enqueue cesarops2 restart only"
    dispatch_handoff "$PEER_NODE" "restart_forge" "primary_forge_down"
    return 0
  fi
  if probe "${FORGE_URL}/health"; then
    return 0
  fi
  local attempt
  for attempt in $(seq 1 "$MAX_TRIES"); do
    log "forge unhealthy on ${NODE} attempt=${attempt}; restarting"
    bash "${REPO}/scripts/forge-health-recover.sh" restart_forge >>"$LOG" 2>&1 || true
    if probe "${FORGE_URL}/health"; then
      log "forge recovered on ${NODE}"
      return 0
    fi
  done
  log "forge still unhealthy on ${NODE}; escalating"
  dispatch_handoff "$NODE" "restart_forge" "local_forge_unhealthy"
  dispatch_handoff "$NODE" "mission_service_watchdog" "local_forge_llm_handoff"
  return 1
}

check_peer_and_handoff() {
  local peer_ok=1
  if ! probe "${PEER_N8N_URL}/healthz"; then
    log "peer n8n unhealthy node=${PEER_NODE}; enqueue remote restart"
    dispatch_handoff "$PEER_NODE" "restart_n8n" "peer_n8n_unhealthy"
    dispatch_handoff "$PEER_NODE" "ensure_n8n_watchdog" "peer_n8n_watchdog_guard"
    peer_ok=0
  fi
  if ! probe "${PEER_FORGE_URL}/health"; then
    log "peer forge unhealthy node=${PEER_NODE}; enqueue remote restart"
    dispatch_handoff "$PEER_NODE" "restart_forge" "peer_forge_unhealthy"
    dispatch_handoff "$PEER_NODE" "ensure_n8n_watchdog" "peer_forge_watchdog_guard"
    peer_ok=0
  fi
  if [[ "$peer_ok" -eq 0 ]]; then
    dispatch_handoff "$PEER_NODE" "mission_service_watchdog" "peer_resilience_followup"
  fi
}

main() {
  mkdir -p "$(dirname "$LOG")"
  ensure_watchdog_guard || true
  ensure_local_n8n || true
  ensure_local_forge || true
  check_peer_and_handoff
}

main "$@"
