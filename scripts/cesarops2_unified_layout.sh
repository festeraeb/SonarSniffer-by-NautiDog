#!/usr/bin/env bash
# cesarops2 unified LLM layout:
#   MIXTRAL_THINKER=1 (default when unified): :5200 RTX+CPU Mixtral thinker
#   else dual-coder-zaya: :5200 RTX draft, :5203 1070 ZAYA thinker
#   :5202 — stopped (frees 1070; was legacy coder)
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
PORT_DRAFT="${PORT_DRAFT:-5200}"
PORT_ZAYA="${PORT_ZAYA:-5203}"

log() { echo "[c2-unified] $*"; }

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}

cmd_start() {
  if [[ "${MIXTRAL_THINKER:-1}" == "1" ]]; then
    log "Mixtral thinker layout (RTX :5200)"
    bash "${REPO}/scripts/cesarops2_mixtral_thinker_layout.sh" start
    return
  fi
  stop_port 5202
  stop_port 5201
  if ! curl -sf --max-time 3 "http://127.0.0.1:${PORT_DRAFT}/v1/models" >/dev/null; then
    log "starting draft :${PORT_DRAFT} (RTX — keep existing Qwen MoE if configured)"
    FLEET_UNIFIED=1 bash "${REPO}/scripts/cesarops2_fleet_roles.sh" start 2>/dev/null || true
  else
    log "draft :${PORT_DRAFT} already up"
  fi
  bash "${REPO}/scripts/zaya/start_zaya_1070.sh"
  for p in "$PORT_DRAFT" "$PORT_ZAYA"; do
    curl -sf --max-time 3 "http://127.0.0.1:${p}/v1/models" >/dev/null \
      && log "OK :${p}" || log "WARN down :${p}"
  done
}

cmd_stop() {
  if [[ "${MIXTRAL_THINKER:-1}" == "1" ]]; then
    bash "${REPO}/scripts/cesarops2_mixtral_thinker_layout.sh" stop
    return
  fi
  stop_port "$PORT_ZAYA"
  stop_port "$PORT_DRAFT"
  stop_port 5202
  log "stopped unified c2 LLM ports"
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status)
    for p in 5200 5203; do
      curl -sf --max-time 2 "http://127.0.0.1:${p}/v1/models" >/dev/null \
        && echo "up :$p" || echo "down :$p"
    done
    ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
