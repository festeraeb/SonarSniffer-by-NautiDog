#!/usr/bin/env bash
# Bootstrap full unified fleet on this host (cesarops2 operator node).
#   bash scripts/fleet_unified_up.sh          # full stack
#   bash scripts/fleet_unified_up.sh --check  # verify only
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/fleet_resolve.sh
source "${SCRIPT_DIR}/lib/fleet_resolve.sh"
# shellcheck source=lib/fleet_unified.sh
source "${SCRIPT_DIR}/lib/fleet_unified.sh"

CHECK_ONLY=0
[[ "${1:-}" == "--check" ]] && CHECK_ONLY=1

log() { echo "[fleet-unified] $*"; }

verify_unified() {
  local fail=0
  probe_http "forge" "${FORGE_URL}/forge/status" | grep -q ok || fail=1
  local n8n_u
  n8n_u="$(fleet_n8n_probe_url)"
  probe_http "n8n" "${n8n_u}/healthz" | grep -q ok || fail=1
  curl -sf --max-time 3 "http://127.0.0.1:5203/v1/models" >/dev/null || { log "DOWN ZAYA :5203"; fail=1; }
  curl -sf --max-time 3 "http://127.0.0.1:5200/v1/models" >/dev/null || { log "DOWN draft :5200"; fail=1; }
  curl -sf --max-time 3 "http://${T440_LAN}:5001/v1/models" >/dev/null || log "warn: T440 :5001 coder down"
  curl -sf --max-time 3 "http://${T440_LAN}:5002/v1/models" >/dev/null || log "warn: T440 :5002 reviewer down"
  return "$fail"
}

if [[ "$CHECK_ONLY" == "1" ]]; then
  fleet_unified_enabled && log "unified mode ON" || log "unified mode OFF"
  probe_mcp_stack
  verify_unified && log "verify OK" || { log "verify FAILED"; exit 1; }
  exit 0
fi

log "enabling unified mode (peer dispatch + shared n8n DB)"
fleet_unified_on

log "MCP sidecars (Context7, Crawl4AI, OpenMemory)"
bash "${REPO}/cesarops-forge-v2/scripts/mcp-stack-up.sh" || log "warn: mcp-stack partial"

if [[ "$FLEET_NODE" == "cesarops2" ]]; then
  log "c2 LLM layout (ZAYA :5203 + draft :5200)"
  FLEET_UNIFIED=1 bash "${REPO}/scripts/cesarops2_unified_layout.sh" start

  log "Forge routing preset dual-coder-zaya"
  FLEET_UNIFIED=1 bash "${REPO}/scripts/forge_apply_dual_coder_zaya.sh"

  if systemctl is-active cesarops-forge-v2.service >/dev/null 2>&1; then
    sudo systemctl restart cesarops-forge-v2.service 2>/dev/null \
      && log "restarted cesarops-forge-v2" \
      || log "forge systemd restart skipped"
  elif [[ -x "${REPO}/start_forge.sh" ]]; then
    bash "${REPO}/start_forge.sh" 2>/dev/null || true
  fi
fi

log "n8n (fleet DB on NFS) + workflow import"
bash "${REPO}/scripts/import_n8n_health_workflows.sh" || bash "${REPO}/scripts/start_n8n.sh" || true

log "watchdogs"
bash "${REPO}/scripts/mission_service_watchdog.sh" || true
bash "${REPO}/scripts/gpu_slot_watchdog.sh" tick 2>/dev/null || true

log "T440 pending jobs (run on c2 against NFS queue)"
run_t440_queue_from_c2

log "local c2 fleet job queue"
FLEET_NODE=cesarops2 bash "${REPO}/scripts/fleet-job-runner.sh" || true

log "route health"
bash "${REPO}/scripts/fleet-route-health.sh" 2>/dev/null || true

log "install timers if root"
if [[ -w /etc/systemd/system ]] || sudo -n true 2>/dev/null; then
  for unit in cesarops-fleet-job-runner-c2.timer cesarops-fleet-job-runner-t440.timer \
    cesarops-fleet-unified.timer; do
    [[ -f "${REPO}/systemd/${unit}" ]] && sudo cp "${REPO}/systemd/${unit}" /etc/systemd/system/ 2>/dev/null || true
    [[ -f "${REPO}/systemd/${unit%.timer}.service" ]] && \
      sudo cp "${REPO}/systemd/${unit%.timer}.service" /etc/systemd/system/ 2>/dev/null || true
  done
  sudo systemctl daemon-reload 2>/dev/null || true
  sudo systemctl enable --now cesarops-fleet-job-runner-c2.timer 2>/dev/null || true
  sudo systemctl enable --now cesarops-fleet-job-runner-t440.timer 2>/dev/null || true
  sudo systemctl enable --now cesarops-fleet-unified.timer 2>/dev/null || true
fi

probe_mcp_stack
verify_unified && log "unified bootstrap complete" || log "bootstrap done with warnings — run: bash scripts/fleet_unified_up.sh --check"
