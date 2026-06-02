#!/usr/bin/env bash
# Execute a Forge recovery action (used by n8n fleet-ops / forge-health-probe).
set -euo pipefail

T440_IP="${T440_IP:-10.0.0.61}"
if [[ "${T440_RECOVERY:-0}" == "1" ]]; then
  FORGE_URL="${FORGE_URL:-http://${T440_IP}:9100}"
else
  FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
fi
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
LOG="${FORGE_RECOVER_LOG:-/data/cesarops/logs/forge-health-recover.log}"
ACTION="${1:-}"

mkdir -p "$(dirname "$LOG")"
ts() { date -Iseconds; }
log() { echo "$(ts) $*" | tee -a "$LOG"; }

case "$ACTION" in
  none|"")
    log "noop"
    exit 0
    ;;
  interrupt)
    log "POST /interrupt"
    curl -sf -X POST "${FORGE_URL}/interrupt" | tee -a "$LOG" || true
    ;;
  clear)
    log "POST /interrupt + /clear"
    curl -sf -X POST "${FORGE_URL}/interrupt" >/dev/null 2>&1 || true
    sleep 2
    curl -sf -X POST "${FORGE_URL}/clear" | tee -a "$LOG" || true
    ;;
  interrupt_and_clear)
    log "interrupt_and_clear"
    curl -sf -X POST "${FORGE_URL}/interrupt" >/dev/null 2>&1 || true
    sleep 3
    curl -sf -X POST "${FORGE_URL}/clear" >/dev/null 2>&1 || true
    ;;
  restart_forge)
    if [[ -f /etc/cesarops/forge-primary-cesarops2 ]] || [[ "$(hostname -s)" == *t440* ]]; then
      log "skip restart_forge: not on T440 (primary http://10.0.0.201:9100)"
      exit 0
    fi
    log "systemctl restart cesarops-forge-v2"
    sudo systemctl restart cesarops-forge-v2
    for i in $(seq 1 30); do
      curl -sf --max-time 3 "${FORGE_URL}/health" >/dev/null 2>&1 && { log "forge up after ${i}*3s"; exit 0; }
      sleep 3
    done
    log "ERROR forge still down after restart"
    exit 1
    ;;
  restart_llama_p100)
    log "p100_gemma_r1_dual restart"
    bash "${REPO}/scripts/p100_gemma_r1_dual.sh" free >>"$LOG" 2>&1 || true
    bash "${REPO}/scripts/p100_gemma_r1_dual.sh" start >>"$LOG" 2>&1
    ;;
  full_recovery)
    if [[ "$(hostname -s)" == *t440* ]]; then
      log "full_recovery on T440: LLM only (no local Forge restart)"
      bash "${REPO}/scripts/p100_gemma_r1_dual.sh" free >>"$LOG" 2>&1 || true
      bash "${REPO}/scripts/p100_gemma_r1_dual.sh" start >>"$LOG" 2>&1 || true
      exit 0
    fi
    log "full_recovery"
    curl -sf -X POST "${FORGE_URL}/interrupt" >/dev/null 2>&1 || true
    sleep 2
    bash "${REPO}/scripts/p100_gemma_r1_dual.sh" free >>"$LOG" 2>&1 || true
    bash "${REPO}/scripts/p100_gemma_r1_dual.sh" start >>"$LOG" 2>&1 || true
    sleep 10
    sudo systemctl restart cesarops-forge-v2
    for i in $(seq 1 30); do
      curl -sf --max-time 3 "${FORGE_URL}/health" >/dev/null 2>&1 && { log "full_recovery forge up"; exit 0; }
      sleep 3
    done
    exit 1
    ;;
  *)
    log "unknown action: $ACTION"
    exit 1
    ;;
esac
