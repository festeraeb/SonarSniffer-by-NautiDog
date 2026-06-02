#!/usr/bin/env bash
# Ensure the n8n watchdog timer is enabled at boot and currently active.
set -euo pipefail

TIMER_UNIT="${N8N_WATCHDOG_TIMER_UNIT:-cesarops-n8n-watchdog.timer}"
SERVICE_UNIT="${N8N_WATCHDOG_SERVICE_UNIT:-cesarops-n8n-watchdog.service}"
MAX_STALE_SECS="${N8N_WATCHDOG_MAX_STALE_SECS:-600}"

log() {
  echo "$(date -Iseconds) [ensure_n8n_watchdog] $*"
}

_run_systemctl() {
  if systemctl "$@" >/dev/null 2>&1; then
    return 0
  fi
  if command -v sudo >/dev/null 2>&1 && sudo -n true >/dev/null 2>&1; then
    sudo -n systemctl "$@" >/dev/null 2>&1
    return $?
  fi
  return 1
}

_is_enabled() {
  systemctl is-enabled "$1" >/dev/null 2>&1
}

_is_active() {
  systemctl is-active "$1" >/dev/null 2>&1
}

_unit_exists() {
  local load_state
  load_state="$(systemctl show "$1" -p LoadState --value 2>/dev/null || true)"
  [[ -n "$load_state" && "$load_state" != "not-found" ]]
}

_last_trigger_epoch() {
  local v
  v="$(systemctl show "$TIMER_UNIT" -p LastTriggerUSec --value 2>/dev/null || true)"
  if [[ -z "$v" || "$v" == "n/a" ]]; then
    echo 0
    return
  fi
  date -d "$v" +%s 2>/dev/null || echo 0
}

main() {
  if ! command -v systemctl >/dev/null 2>&1; then
    log "systemctl not found; cannot verify watchdog startup"
    return 1
  fi

  local ok=0

  if ! _is_enabled "$TIMER_UNIT"; then
    log "$TIMER_UNIT not enabled; enabling"
    if ! _run_systemctl enable "$TIMER_UNIT"; then
      log "failed to enable $TIMER_UNIT"
      ok=1
    fi
  fi

  if ! _is_active "$TIMER_UNIT"; then
    log "$TIMER_UNIT not active; starting"
    if ! _run_systemctl start "$TIMER_UNIT"; then
      log "failed to start $TIMER_UNIT"
      ok=1
    fi
  fi

  # Service is oneshot and may be inactive between timer ticks; only verify unit exists.
  if ! _unit_exists "$SERVICE_UNIT"; then
    log "$SERVICE_UNIT missing or inaccessible"
    ok=1
  fi

  if _is_active "$TIMER_UNIT"; then
    local now_epoch
    local last_epoch
    local age
    now_epoch="$(date +%s)"
    last_epoch="$(_last_trigger_epoch)"

    if [[ "$last_epoch" -le 0 ]]; then
      # Do not start $SERVICE_UNIT from this helper; it may be called by the
      # service itself and can recurse/block startup.
      log "$TIMER_UNIT has no trigger history yet; waiting for first timer tick"
    else
      age=$((now_epoch - last_epoch))
      if [[ "$age" -gt "$MAX_STALE_SECS" ]]; then
        log "$TIMER_UNIT trigger stale (${age}s > ${MAX_STALE_SECS}s); restarting timer only"
        if ! _run_systemctl restart "$TIMER_UNIT"; then
          log "failed to restart $TIMER_UNIT"
          ok=1
        fi
      fi
    fi
  fi

  if [[ "$ok" -ne 0 ]]; then
    return 1
  fi

  log "$TIMER_UNIT enabled+active; watchdog boot/liveness guard OK"
}

main "$@"
