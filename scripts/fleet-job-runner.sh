#!/usr/bin/env bash
# Process fleet ops jobs from the NFS queue (one host per run).
# Install on each node: systemd/cesarops-fleet-job-runner.timer (optional).
set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/fleet_resolve.sh
source "${_SCRIPT_DIR}/lib/fleet_resolve.sh"
NODE="${FLEET_NODE}"

QUEUE="${REPO}/var/fleet-jobs"
PENDING="${QUEUE}/pending/${NODE}"
RUNNING="${QUEUE}/running/${NODE}"
DONE="${QUEUE}/done/${NODE}"
FAILED="${QUEUE}/failed/${NODE}"

mkdir -p "$PENDING" "$RUNNING" "$DONE" "$FAILED"

run_action() {
  local action="$1"
  shift
  case "$action" in
    prep_post_pipeline)
      bash "${REPO}/scripts/prep_post_pipeline.sh"
      ;;
    install_cake)
      bash "${REPO}/scripts/install_cake_fleet.sh"
      ;;
    cake_worker_start)
      bash "${REPO}/scripts/cake/start-worker-local.sh"
      ;;
    cake_worker_stop)
      pkill -f 'cake worker.*cluster-key' 2>/dev/null || true
      pkill -f 'cake run.*cluster-key' 2>/dev/null || true
      ;;
    cake_fleet_start)
      USE_70B="${USE_70B:-0}" bash "${REPO}/scripts/cake/start-fleet-cluster.sh"
      ;;
    cake_fleet_stop)
      bash "${REPO}/scripts/cake/stop-fleet-cluster.sh"
      ;;
    fleet_wake)
      bash "${REPO}/scripts/cesarops-fleet-mode.sh" wake
      ;;
    sync_llm_endpoints)
      bash "${REPO}/scripts/fleet-sync-cesarops2-llm.sh"
      ;;
    cake_pull_models)
      bash "${REPO}/scripts/cake_pull_fleet_models.sh"
      ;;
    restart_n8n)
      bash "${REPO}/scripts/start_n8n.sh"
      ;;
    import_n8n_health_workflows)
      bash "${REPO}/scripts/import_n8n_health_workflows.sh"
      ;;
    start_zaya_1070)
      bash "${REPO}/scripts/zaya/start_zaya_1070.sh"
      ;;
    unified_up)
      bash "${REPO}/scripts/fleet_unified_up.sh"
      ;;
    route_health_check)
      bash "${REPO}/scripts/fleet-route-health.sh"
      ;;
    forge_health_probe)
      bash "${REPO}/scripts/forge-health-probe.sh"
      ;;
    forge_interrupt)
      bash "${REPO}/scripts/forge-health-recover.sh" interrupt
      ;;
    forge_clear_busy)
      bash "${REPO}/scripts/forge-health-recover.sh" interrupt_and_clear
      ;;
    restart_forge)
      if [[ "$NODE" == "t440" ]]; then
        echo "[fleet] skip restart_forge on T440 — primary Forge is cesarops2"
        exit 0
      fi
      bash "${REPO}/scripts/forge-health-recover.sh" restart_forge
      ;;
    ensure_n8n_watchdog)
      bash "${REPO}/scripts/ensure_n8n_watchdog.sh"
      ;;
    restart_llama_p100)
      bash "${REPO}/scripts/forge-health-recover.sh" restart_llama_p100
      ;;
    forge_full_recovery)
      bash "${REPO}/scripts/forge-health-recover.sh" full_recovery
      ;;
    mission_service_watchdog)
      bash "${REPO}/scripts/mission_service_watchdog.sh"
      ;;
    start_zaya_p100_vulkan)
      bash "${REPO}/scripts/zaya/start_zaya_p100_vulkan.sh"
      ;;
    resilience_tick)
      bash "${REPO}/scripts/n8n-watchdog.sh"
      ;;
    blueprint_audit_deep)
      python3 "${REPO}/scripts/blueprint_audit_fleet_dispatch.py" \
        --repo "${REPO}" --parallel "${BLUEPRINT_AUDIT_PARALLEL:-3}"
      ;;
    t440_remove_forge)
      bash "${REPO}/scripts/cluster-exec-t440.sh" t440-remove-forge
      ;;
    script_inventory_scan)
      bash "${REPO}/scripts/scan-script-inventory.sh" --out "${REPO}/var/script-inventory/latest"
      ;;
    script_cleanup_ephemeral)
      DRY_RUN="${DRY_RUN:-1}" bash "${REPO}/scripts/cleanup-ephemeral-scripts.sh"
      ;;
    *)
      echo "unknown action: $action" >&2
      return 1
      ;;
  esac
}

process_one() {
  local job="$1"
  local base
  base=$(basename "$job" .json)
  local dest="${RUNNING}/${base}.json"
  mv "$job" "$dest"
  local action
  action=$(python3 -c "import json; print(json.load(open('$dest')).get('action',''))" 2>/dev/null || echo "")
  local log="${DONE}/${base}.log"
  if run_action "$action" >"$log" 2>&1; then
    mv "$dest" "${DONE}/${base}.json"
    echo "{\"ok\":true,\"node\":\"$NODE\",\"action\":\"$action\",\"log\":\"$log\"}" >"${DONE}/${base}.status.json"
  else
    mv "$dest" "${FAILED}/${base}.json"
    echo "{\"ok\":false,\"node\":\"$NODE\",\"action\":\"$action\",\"log\":\"$log\"}" >"${FAILED}/${base}.status.json"
  fi
}

shopt -s nullglob
for job in "$PENDING"/*.json; do
  [[ -f "$job" ]] || continue
  process_one "$job"
done
