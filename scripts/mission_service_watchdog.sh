#!/usr/bin/env bash
# Mission service watchdog:
# - Ensures n8n + Forge are alive
# - Discovers healthy compute endpoints (Forge + NautiInferer + static pools)
# - Repoints Forge routing to available endpoints
# - Attempts local CPU/LLM recovery when pools collapse
set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/fleet_resolve.sh
source "${_SCRIPT_DIR}/lib/fleet_resolve.sh"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
N8N_URL="${N8N_URL:-http://127.0.0.1:5678}"
NAUTI_URL="${NAUTI_URL:-http://127.0.0.1:8099}"
DISCOVERY_JSON="${DISCOVERY_JSON:-/tmp/compute_sources_last.json}"
LOG="${MISSION_WATCHDOG_LOG:-/data/cesarops/logs/mission-service-watchdog.log}"
LOCK="${MISSION_WATCHDOG_LOCK:-/tmp/mission-service-watchdog.lock}"
EXTRA_ENDPOINTS="${EXTRA_LLM_ENDPOINTS:-}"
ISOLATION_MARK="${CESAROPS2_ISOLATION_MARK:-$HOME/.cache/cesarops/cesarops2-isolated}"
C2_LAN_HOST="${CESAROPS2_LAN_HOST:-10.0.0.201}"
# Legacy presets — not used for auto-recover when GPU_SLOT_DYNAMIC=1 (default).
C2_TRIPLE_SCRIPT="${C2_TRIPLE_SCRIPT:-${REPO}/scripts/cesarops2_triple_gpu_llama.sh}"
T440_DUAL_SCRIPT="${T440_DUAL_SCRIPT:-${REPO}/scripts/p100_gemma_r1_dual.sh}"
GPU_SLOT_DYNAMIC="${GPU_SLOT_DYNAMIC:-1}"
GPU_SLOT_HEARTBEAT_PATH="${GPU_SLOT_HEARTBEAT_PATH:-/data/cesarops/logs/gpu-slot-heartbeat.json}"
GPU_SLOT_TICK_SCRIPT="${GPU_SLOT_TICK_SCRIPT:-${REPO}/scripts/gpu_slot_heartbeat.py}"
C2_CPU_SIM_SCRIPT="${C2_CPU_SIM_SCRIPT:-${REPO}/cesarops-detection/workers/cpu_sim_workers.py}"
C2_CPU_SIM_PY="${C2_CPU_SIM_PY:-/home/cesarops/.venvs/cesarops-lab/bin/python}"
FORGE_ROUTING_STATE="${FORGE_ROUTING_STATE:-${REPO}/cesarops-forge-v2/routing_state.json}"
FORGE_CLUSTER_CONFIG="${FORGE_CLUSTER_CONFIG:-${REPO}/cesarops-forge-v2/cluster_config.toml}"
NAUTIVECS_HEALTH_URL="${NAUTIVECS_HEALTH_URL:-http://127.0.0.1:5003/health}"
NAUTIVECS_QUERY_URL="${NAUTIVECS_QUERY_URL:-http://127.0.0.1:5003/query}"
NAUTIVECS_SERVICE="${NAUTIVECS_SERVICE:-cesarops-nautivecs.service}"
N8N_ACTIVATE_SCRIPT="${N8N_ACTIVATE_SCRIPT:-${REPO}/scripts/n8n_activate_fleet_workflows.sh}"
N8N_IMPORT_SCRIPT="${N8N_IMPORT_SCRIPT:-${REPO}/scripts/import_n8n_health_workflows.sh}"
N8N_DB="${N8N_DB:-}"
# Force-heal local card stacks on known hosts (t440 / cesarops2) every tick.
LOCAL_CARD_AUTO_RECOVER="${LOCAL_CARD_AUTO_RECOVER:-1}"
# Legacy coarse toggle for rebind behavior.
# 1 = allow auto-switch policy to run, 0 = force policy to off unless overridden.
C2_AUTO_REBIND="${C2_AUTO_REBIND:-1}"
# Auto-switch policy for c2 recovery: off | idle_only | always.
# If unset, falls back to legacy C2_AUTO_REBIND mapping.
C2_AUTO_SWITCH_MODE="${C2_AUTO_SWITCH_MODE:-}"
# Set to 1 to enforce expected GPU/card mapping for watched ports.
C2_ENFORCE_GPU_MATCH="${C2_ENFORCE_GPU_MATCH:-0}"
# Optional override: comma-separated list like "5200:RTX 2060 SUPER,5300:".
C2_WATCH_PORT_SPECS="${C2_WATCH_PORT_SPECS:-}"
# If 1, reroute Forge dynamically when c2 node ports are unrecoverable.
C2_REROUTE_ON_UNRECOVERABLE="${C2_REROUTE_ON_UNRECOVERABLE:-1}"
# Operator acknowledgement file for busy-time rebind overrides.
C2_REBIND_ACK_FILE="${C2_REBIND_ACK_FILE:-/tmp/c2-rebind.ack}"
C2_REBIND_PENDING_FILE="${C2_REBIND_PENDING_FILE:-/tmp/c2-rebind.pending.json}"
C2_REBIND_ACK_TTL_SEC="${C2_REBIND_ACK_TTL_SEC:-900}"
# Set to 1 to let watchdog rewrite Forge routing each tick.
MISSION_DYNAMIC_ROUTE_UPDATE="${MISSION_DYNAMIC_ROUTE_UPDATE:-0}"

# Tick-scoped state used by main() to decide if forced reroute should run.
C2_UNRECOVERABLE_NODE=0

if [[ -z "$C2_AUTO_SWITCH_MODE" ]]; then
  C2_AUTO_SWITCH_MODE="idle_only"
fi
if [[ "$C2_AUTO_REBIND" != "1" && "$C2_AUTO_SWITCH_MODE" == "idle_only" ]]; then
  C2_AUTO_SWITCH_MODE="off"
fi

mkdir -p "$(dirname "$LOG")"

# Scan-safe guard: when present, watchdog exits immediately.
WATCHDOG_GUARD="${CESAROPS_SCAN_NO_WATCHDOG_GUARD:-/tmp/cesarops-scan-no-watchdog}"
if [[ -f "$WATCHDOG_GUARD" ]]; then
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] scan-safe guard present ($WATCHDOG_GUARD); skipping mission_service_watchdog" \
    | tee -a "$LOG" 2>/dev/null || true
  exit 0
fi

log() {
  local msg="[$(date '+%Y-%m-%d %H:%M:%S')] $*"
  echo "$msg" | tee -a "$LOG"
}

with_lock() {
  exec 9>"$LOCK"
  if ! flock -n 9; then
    log "watchdog already running; exiting"
    exit 0
  fi
}

probe_url() {
  local url="$1"
  curl -sf --max-time 4 "$url" >/dev/null 2>&1
}

post_json_code() {
  local url="$1"
  local body="$2"
  curl -s -o /dev/null -w '%{http_code}' --max-time 5 -X POST \
    -H 'Content-Type: application/json' \
    -d "$body" "$url" 2>/dev/null || echo "000"
}

llm_models_ok() {
  local base="$1"
  curl -sf --max-time 5 "${base%/}/v1/models" >/dev/null 2>&1
}

run_systemctl_any() {
  if systemctl --user "$@" >/dev/null 2>&1; then
    return 0
  fi
  if systemctl "$@" >/dev/null 2>&1; then
    return 0
  fi
  if command -v sudo >/dev/null 2>&1 && sudo -n true >/dev/null 2>&1; then
    sudo -n systemctl "$@" >/dev/null 2>&1
    return $?
  fi
  return 1
}

is_cesarops2_host() {
  local hn
  hn="$(hostname -s | tr '[:upper:]' '[:lower:]')"
  [[ "$hn" != *t440* ]]
}

is_t440_host() {
  local hn
  hn="$(hostname -s | tr '[:upper:]' '[:lower:]')"
  [[ "$hn" == *t440* ]]
}

isolation_mode_on() {
  [[ -f "$ISOLATION_MARK" ]]
}

pid_for_port() {
  local port="$1"
  ss -ltnp | awk -v p=":${port} " '$0 ~ p {print $NF}' | sed -E 's/.*pid=([0-9]+).*/\1/' | head -n1
}

port_reports_expected_gpu() {
  local port="$1"
  local expected="$2"
  local pid
  pid="$(pid_for_port "$port")"
  [[ -n "$pid" ]] || return 1
  nvidia-smi --query-compute-apps=pid,gpu_uuid --format=csv,noheader 2>/dev/null | \
    awk -F', ' -v p="$pid" '$1==p{print $2}' | \
    while IFS= read -r uuid; do
      nvidia-smi --query-gpu=gpu_uuid,name --format=csv,noheader 2>/dev/null | \
        awk -F', ' -v u="$uuid" '$1==u{print $2}'
    done | grep -Fq "$expected"
}

expected_gpu_for_port() {
  case "$1" in
  5200) echo "RTX 2060 SUPER" ;;
  5201) echo "P106-100" ;;
  5202|5571) echo "GTX 1070" ;;
  *) echo "" ;;
  esac
}

watch_port_specs() {
  if [[ -n "$C2_WATCH_PORT_SPECS" ]]; then
    echo "$C2_WATCH_PORT_SPECS" | tr ',' '\n'
    return 0
  fi

  python3 - "$FORGE_ROUTING_STATE" "$C2_LAN_HOST" <<'PY'
import json
import pathlib
import sys
from urllib.parse import urlparse

state_path = pathlib.Path(sys.argv[1])
c2_host = sys.argv[2]
if not state_path.exists():
  raise SystemExit

try:
  data = json.loads(state_path.read_text(encoding="utf-8"))
except Exception:
  raise SystemExit

keys = (
  "reviewer_endpoint",
  "corrector_endpoint",
  "draft_endpoint",
  "thinker_endpoint",
  "coder_endpoint",
)
ports = set()
for key in keys:
  url = str(data.get(key) or "").strip()
  if not url:
    continue
  try:
    p = urlparse(url)
    host = (p.hostname or "").lower()
    port = int(p.port) if p.port else None
  except Exception:
    continue
  if not port:
    continue
  if host not in {"127.0.0.1", "localhost", c2_host.lower()}:
    continue
  if 5200 <= port <= 5999:
    ports.add(port)

for port in sorted(ports):
  print(port)
PY
}

forge_has_active_tasks() {
  local status_json missions_json
  status_json="$(curl -sf --max-time 4 "${FORGE_URL}/forge/status" 2>/dev/null || true)"
  missions_json="$(curl -sf --max-time 4 "${FORGE_URL}/webhook/missions" 2>/dev/null || true)"

  python3 - "$status_json" "$missions_json" <<'PY'
import json
import sys

busy = False
running = False

try:
  raw = sys.argv[1].strip()
  if raw:
    s = json.loads(raw)
    busy = bool(s.get("send_busy"))
except Exception:
  pass

try:
  raw = sys.argv[2].strip()
  if raw:
    m = json.loads(raw)
    if isinstance(m, list):
      arr = m
    elif isinstance(m, dict) and isinstance(m.get("missions"), list):
      arr = m.get("missions", [])
    else:
      arr = []
    for item in arr:
      if isinstance(item, dict) and str(item.get("status", "")).lower() == "running":
        running = True
        break
except Exception:
  pass

print("1" if (busy or running) else "0")
PY
}

has_fresh_rebind_ack() {
  [[ -f "$C2_REBIND_ACK_FILE" ]] || return 1
  local now mtime age
  now="$(date +%s)"
  mtime="$(stat -c %Y "$C2_REBIND_ACK_FILE" 2>/dev/null || echo 0)"
  age=$((now - mtime))
  [[ "$age" -ge 0 && "$age" -le "$C2_REBIND_ACK_TTL_SEC" ]]
}

write_rebind_pending() {
  local reason="$1"
  local specs="$2"
  python3 - "$C2_REBIND_PENDING_FILE" "$reason" "$specs" <<'PY'
import json
import pathlib
import sys
import time

path = pathlib.Path(sys.argv[1])
reason = sys.argv[2]
specs = [s.strip() for s in (sys.argv[3] or "").splitlines() if s.strip()]
payload = {
  "created_at": int(time.time()),
  "reason": reason,
  "requested_ports": specs,
  "action": "touch ack file to allow immediate rebind",
}
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
PY
}

all_ports_healthy() {
  local p
  for p in "$@"; do
    if ! probe_url "http://127.0.0.1:${p}/health"; then
      return 1
    fi
  done
  return 0
}

gpu_slot_recover_allowed() {
  if isolation_mode_on; then
    return 1
  fi
  local in_use ack_ok do_rebind=0
  in_use="$(forge_has_active_tasks || echo 0)"
  ack_ok=0
  has_fresh_rebind_ack && ack_ok=1
  case "${C2_AUTO_SWITCH_MODE,,}" in
    always)
      [[ "$in_use" == "1" && "$ack_ok" != "1" ]] && return 1
      return 0
      ;;
    idle_only)
      [[ "$in_use" == "1" && "$ack_ok" != "1" ]] && return 1
      return 0
      ;;
    off|*)
      [[ "$ack_ok" == "1" ]] && return 0
      return 1
      ;;
  esac
}

ensure_gpu_slots_from_heartbeat() {
  if [[ "$LOCAL_CARD_AUTO_RECOVER" != "1" ]]; then
    return 0
  fi
  if [[ "$GPU_SLOT_DYNAMIC" != "1" ]]; then
    ensure_local_card_stack_legacy
    return 0
  fi
  if [[ ! -f "$GPU_SLOT_TICK_SCRIPT" ]]; then
    log "warn: missing $GPU_SLOT_TICK_SCRIPT — falling back to legacy stack scripts"
    ensure_local_card_stack_legacy
    return 0
  fi
  local extra=()
  if ! gpu_slot_recover_allowed; then
    extra+=(--no-recover)
    log "gpu-slot: record-only (forge busy / policy)"
  fi
  log "gpu-slot: heartbeat tick (dynamic restore, not triple-stack preset)"
  REPO="$REPO" FORGE_URL="$FORGE_URL" FORGE_ROUTING_STATE="$FORGE_ROUTING_STATE" \
    GPU_SLOT_HEARTBEAT_PATH="$GPU_SLOT_HEARTBEAT_PATH" CESAROPS2_LAN_HOST="$C2_LAN_HOST" \
    python3 "$GPU_SLOT_TICK_SCRIPT" tick "${extra[@]}" >>"$LOG" 2>&1 || \
    log "warn: gpu_slot_heartbeat tick failed"
}

ensure_local_card_stack_legacy() {
  if is_t440_host; then
    if all_ports_healthy 5001 5002; then
      return 0
    fi
    if [[ -x "$T440_DUAL_SCRIPT" ]]; then
      log "legacy: t440 dual P100 stack on 5001/5002"
      bash "$T440_DUAL_SCRIPT" start >>"$LOG" 2>&1 || log "warn: t440 dual stack start failed"
      sleep 3
    fi
    return 0
  fi
  if is_cesarops2_host; then
    if all_ports_healthy 5200 5201 5202; then
      return 0
    fi
    if [[ -x "$C2_TRIPLE_SCRIPT" ]]; then
      log "legacy: cesarops2 triple GPU stack on 5200/5201/5202"
      bash "$C2_TRIPLE_SCRIPT" start >>"$LOG" 2>&1 || log "warn: cesarops2 triple stack start failed"
      sleep 3
    fi
  fi
}

ensure_c2_cards_reporting() {
  if ! is_cesarops2_host; then
    return 0
  fi
  if isolation_mode_on; then
    log "isolation mode ON; skipping c2 card/port rebind"
    return 0
  fi
  if [[ "$GPU_SLOT_DYNAMIC" != "1" && ! -x "$C2_TRIPLE_SCRIPT" ]]; then
    log "c2 triple script missing: $C2_TRIPLE_SCRIPT"
    return 1
  fi

  local need_rebind=0
  local specs
  specs="$(watch_port_specs || true)"
  if [[ -z "${specs//[[:space:]]/}" && -f "$FORGE_ROUTING_STATE" ]]; then
    log "no c2 endpoints found in Forge routing; skipping c2 reconciliation"
    return 0
  fi
  if [[ -z "${specs//[[:space:]]/}" ]]; then
    specs=$'5200\n5201\n5202'
  fi

  local spec port card
  while IFS= read -r spec; do
    [[ -n "$spec" ]] || continue
    spec="${spec// /}"
    if [[ "$spec" == *:* ]]; then
      port="${spec%%:*}"
      card="${spec#*:}"
    else
      port="$spec"
      card="$(expected_gpu_for_port "$port")"
    fi
    [[ "$port" =~ ^[0-9]+$ ]] || continue

    if ! probe_url "http://127.0.0.1:${port}/health"; then
      log "port ${port} health is down"
      need_rebind=1
      C2_UNRECOVERABLE_NODE=1
      continue
    fi
    if [[ -n "$card" ]] && ! port_reports_expected_gpu "$port" "$card"; then
      if [[ "$C2_ENFORCE_GPU_MATCH" == "1" ]]; then
        log "port ${port} does not report expected gpu: ${card}"
        need_rebind=1
      else
        log "port ${port} gpu mismatch (expected ${card}) — warn only"
      fi
    fi
  done <<< "$specs"

  if [[ "$need_rebind" -eq 1 ]]; then
    local in_use ack_ok do_rebind reason
    in_use="$(forge_has_active_tasks || echo 0)"
    ack_ok=0
    has_fresh_rebind_ack && ack_ok=1

    do_rebind=0
    reason="policy_block"
    case "${C2_AUTO_SWITCH_MODE,,}" in
      always)
        if [[ "$in_use" == "1" && "$ack_ok" != "1" ]]; then
          reason="busy_no_ack"
        else
          do_rebind=1
          reason="policy_always"
        fi
        ;;
      idle_only)
        if [[ "$in_use" == "1" ]]; then
          [[ "$ack_ok" == "1" ]] && { do_rebind=1; reason="busy_with_ack"; } || reason="busy_no_ack"
        else
          do_rebind=1
          reason="idle_auto"
        fi
        ;;
      off|*)
        [[ "$ack_ok" == "1" ]] && { do_rebind=1; reason="manual_ack"; } || reason="autoswitch_off"
        ;;
    esac

    if [[ "$do_rebind" == "1" ]]; then
      if [[ "$GPU_SLOT_DYNAMIC" == "1" && -f "$GPU_SLOT_TICK_SCRIPT" ]]; then
        log "rebind approved (${reason}): gpu-slot heartbeat restore (not triple-stack)"
        REPO="$REPO" FORGE_URL="$FORGE_URL" FORGE_ROUTING_STATE="$FORGE_ROUTING_STATE" \
          GPU_SLOT_HEARTBEAT_PATH="$GPU_SLOT_HEARTBEAT_PATH" CESAROPS2_LAN_HOST="$C2_LAN_HOST" \
          python3 "$GPU_SLOT_TICK_SCRIPT" tick >>"$LOG" 2>&1 || true
      else
        log "rebind approved (${reason}): legacy triple GPU reconciliation"
        bash "$C2_TRIPLE_SCRIPT" start >>"$LOG" 2>&1 || true
      fi
      sleep 3
      [[ "$ack_ok" == "1" ]] && rm -f "$C2_REBIND_ACK_FILE" >/dev/null 2>&1 || true
    else
      write_rebind_pending "$reason" "$specs"
      log "rebind deferred (${reason}): mode=${C2_AUTO_SWITCH_MODE} in_use=${in_use} ack=${ack_ok}; pending at $C2_REBIND_PENDING_FILE"
      log "ack now with: touch $C2_REBIND_ACK_FILE"
    fi
  fi

  # Ensure vision worker endpoints are reachable from LAN so Forge can observe them.
  if ! probe_url "http://${C2_LAN_HOST}:5570/health" || ! probe_url "http://${C2_LAN_HOST}:5572/health"; then
    if [[ -x "$C2_CPU_SIM_PY" && -f "$C2_CPU_SIM_SCRIPT" ]]; then
      log "vision worker ports not reachable on LAN; rebinding cpu sim workers to 0.0.0.0"
      pkill -f 'cpu_sim_workers.py' >/dev/null 2>&1 || true
      nohup "$C2_CPU_SIM_PY" "$C2_CPU_SIM_SCRIPT" all --host 0.0.0.0 >>"$LOG" 2>&1 &
      sleep 2
    else
      log "cpu sim worker launcher unavailable; skipping rebind"
    fi
  fi

  # Snapshot card reporting each tick for watchdog evidence.
  nvidia-smi --query-gpu=index,name,utilization.gpu,memory.used,memory.total --format=csv,noheader >>"$LOG" 2>/dev/null || true
}

ensure_n8n() {
  if probe_url "${N8N_URL}/healthz"; then
    return 0
  fi
  log "n8n unhealthy; restarting"
  bash "${REPO}/scripts/start_n8n.sh" >>"$LOG" 2>&1 || true
}

ensure_forge() {
  local hn
  hn="$(hostname -s | tr '[:upper:]' '[:lower:]')"
  if [[ "$hn" == *t440* ]]; then
    return 0
  fi
  if probe_url "${FORGE_URL}/health"; then
    return 0
  fi
  log "forge unhealthy; restarting"
  bash "${REPO}/scripts/forge-health-recover.sh" restart_forge >>"$LOG" 2>&1 || true
}

n8n_webhook_code() {
  local path="$1"
  local url body
  url="${N8N_URL%/}/webhook/${path}"
  case "$path" in
    fleet-ops) body='{"node":"t440","action":"route_health_check","probe":true}' ;;
    tool-route|pamp-route|llm-translate) body='{"message":"watchdog probe","probe":true}' ;;
    prompt-tuner-initial|prompt-tuner-failed) body='{"probe":true,"lane":"watchdog"}' ;;
    *) body='{"probe":true}' ;;
  esac
  post_json_code "$url" "$body"
}

webhook_ok() {
  local code="$1"
  [[ "$code" =~ ^[23] ]] || [[ "$code" == "400" ]]
}

ensure_n8n_core_workflows() {
  local paths=(
    "fleet-ops"
    "tool-route"
    "llm-translate"
    "pamp-route"
    "prompt-tuner-initial"
    "prompt-tuner-failed"
  )
  local p code missing=()

  for p in "${paths[@]}"; do
    code="$(n8n_webhook_code "$p")"
    if ! webhook_ok "$code"; then
      missing+=("${p}:${code}")
    fi
  done

  if [[ "${#missing[@]}" -eq 0 ]]; then
    return 0
  fi

  log "n8n core webhooks missing: ${missing[*]} ; activating workflows"
  if [[ -x "$N8N_ACTIVATE_SCRIPT" ]]; then
    if [[ -n "$N8N_DB" ]]; then
      N8N_DB="$N8N_DB" bash "$N8N_ACTIVATE_SCRIPT" >>"$LOG" 2>&1 || log "warn: n8n workflow activation failed"
    else
      bash "$N8N_ACTIVATE_SCRIPT" >>"$LOG" 2>&1 || log "warn: n8n workflow activation failed"
    fi
  fi

  missing=()
  for p in "${paths[@]}"; do
    code="$(n8n_webhook_code "$p")"
    if ! webhook_ok "$code"; then
      missing+=("${p}:${code}")
    fi
  done
  if [[ "${#missing[@]}" -eq 0 ]]; then
    return 0
  fi

  if [[ -x "$N8N_IMPORT_SCRIPT" ]]; then
    log "n8n webhooks still missing; importing canonical workflow set"
    if [[ -n "$N8N_DB" ]]; then
      N8N_DB="$N8N_DB" bash "$N8N_IMPORT_SCRIPT" >>"$LOG" 2>&1 || log "warn: n8n workflow import failed"
    else
      bash "$N8N_IMPORT_SCRIPT" >>"$LOG" 2>&1 || log "warn: n8n workflow import failed"
    fi
  fi

  missing=()
  for p in "${paths[@]}"; do
    code="$(n8n_webhook_code "$p")"
    if ! webhook_ok "$code"; then
      missing+=("${p}:${code}")
    fi
  done
  if [[ "${#missing[@]}" -eq 0 ]]; then
    return 0
  fi

  log "warn: n8n core webhooks still unavailable after repair: ${missing[*]}"
  return 1
}

nautivecs_query_ok() {
  local code
  code="$(post_json_code "$NAUTIVECS_QUERY_URL" '{"query":"forge watchdog probe","top_k":1}')"
  [[ "$code" =~ ^[23] ]] || [[ "$code" == "400" ]]
}

ensure_nautivecs_memory() {
  if probe_url "$NAUTIVECS_HEALTH_URL" && nautivecs_query_ok; then
    return 0
  fi

  log "nautivecs unhealthy; restarting ${NAUTIVECS_SERVICE}"
  run_systemctl_any restart "$NAUTIVECS_SERVICE" || run_systemctl_any start "$NAUTIVECS_SERVICE" || \
    log "warn: unable to restart ${NAUTIVECS_SERVICE}"
  sleep 2

  if probe_url "$NAUTIVECS_HEALTH_URL" && nautivecs_query_ok; then
    return 0
  fi

  log "warn: nautivecs health/query still failing after repair"
  return 1
}

bootstrap_config_is_expected() {
  python3 - "$FORGE_CLUSTER_CONFIG" "$C2_LAN_HOST" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
host = sys.argv[2]
if not path.exists():
    raise SystemExit(1)

text = path.read_text(encoding="utf-8")
checks = [
    re.search(r"(?ms)^\[bootstrap\].*?^port\s*=\s*5201\s*$", text),
    re.search(rf'(?ms)^\[nicknames\].*?^bootstrap\s*=\s*"http://{re.escape(host)}:5201"\s*$', text),
    re.search(rf'(?ms)^\[roles\].*?^bootstrap\s*=\s*"http://{re.escape(host)}:5201"\s*$', text),
]
raise SystemExit(0 if all(checks) else 1)
PY
}

repair_bootstrap_config() {
  python3 - "$FORGE_CLUSTER_CONFIG" "$C2_LAN_HOST" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
host = sys.argv[2]
text = path.read_text(encoding="utf-8")
text = re.sub(r'(?ms)(^\[bootstrap\].*?^port\s*=\s*)\d+\s*$', r'\g<1>5201', text)
text = re.sub(r'(?ms)(^\[nicknames\].*?^bootstrap\s*=\s*)"[^"]+"\s*$', rf'\g<1>"http://{host}:5201"', text)
text = re.sub(r'(?ms)(^\[roles\].*?^bootstrap\s*=\s*)"[^"]+"\s*$', rf'\g<1>"http://{host}:5201"', text)
path.write_text(text, encoding="utf-8")
PY
}

ensure_bootstrap_mapping() {
  local expected="http://${C2_LAN_HOST}:5201"
  if bootstrap_config_is_expected && llm_models_ok "$expected"; then
    return 0
  fi
  if ! llm_models_ok "$expected"; then
    log "warn: bootstrap fallback ${expected} is not serving /v1/models"
    return 1
  fi

  log "bootstrap mapping drift detected; restoring bootstrap to ${expected}"
  repair_bootstrap_config
  bash "${REPO}/scripts/forge-health-recover.sh" restart_forge >>"$LOG" 2>&1 || \
    log "warn: forge restart after bootstrap repair failed"
}

ensure_runtime_corrections() {
  ensure_n8n_core_workflows || true
  ensure_nautivecs_memory || true
  ensure_bootstrap_mapping || true
}

discover_compute() {
  python3 "${REPO}/scripts/discover_compute_sources.py" \
    --forge-url "$FORGE_URL" \
    --nauti-url "$NAUTI_URL" \
    --extra-endpoints "$EXTRA_ENDPOINTS" \
    >"$DISCOVERY_JSON"
}

read_pool_value() {
  local key="$1"
  python3 - "$DISCOVERY_JSON" "$key" <<'PY'
import json, sys, pathlib
p = pathlib.Path(sys.argv[1])
key = sys.argv[2]
if not p.exists():
    print("")
    raise SystemExit
j = json.loads(p.read_text(encoding='utf-8'))
arr = j.get('pools', {}).get(key) or []
print(arr[0] if arr else "")
PY
}

healthy_count() {
  python3 - "$DISCOVERY_JSON" <<'PY'
import json, sys, pathlib
p = pathlib.Path(sys.argv[1])
if not p.exists():
    print(0)
    raise SystemExit
j = json.loads(p.read_text(encoding='utf-8'))
print(int(j.get('healthy_nodes') or 0))
PY
}

route_to_available() {
  local thinker reviewer corrector coder draft
  thinker="$(read_pool_value thinker)"
  reviewer="$(read_pool_value reviewer)"
  corrector="$(read_pool_value corrector)"
  coder="$(read_pool_value coder)"
  draft="$corrector"

  if [[ -z "$thinker" || -z "$reviewer" || -z "$corrector" ]]; then
    log "routing skipped: incomplete endpoint pool"
    return 1
  fi
  if [[ -z "$coder" ]]; then
    coder="http://127.0.0.1:5001"
  fi

  if ! probe_url "${FORGE_URL}/health"; then
    log "routing skipped: forge unavailable"
    return 1
  fi

  log "applying dynamic routing coder=$coder thinker=$thinker reviewer=$reviewer corrector=$corrector draft=$draft"
  curl -sf -X POST "${FORGE_URL}/cluster/routing" \
    -H 'Content-Type: application/json' \
    -d "{\"coder_endpoint\":\"${coder}\",\"thinker_endpoint\":\"${thinker}\",\"reviewer_endpoint\":\"${reviewer}\",\"corrector_endpoint\":\"${corrector}\",\"draft_endpoint\":\"${draft}\"}" \
    >/dev/null || return 1
  return 0
}

recover_if_no_compute() {
  local n
  n="$(healthy_count)"
  if [[ "$n" -gt 0 ]]; then
    return 0
  fi

  log "no healthy compute endpoints discovered; attempting recovery"

  # Try local dual P100 stack first.
  bash "${REPO}/scripts/p100_gemma_r1_dual.sh" start >>"$LOG" 2>&1 || true

  # Try NautiInferer coordinator if present.
  bash "${REPO}/scripts/nauti-inferer-c2.sh" start >>"$LOG" 2>&1 || true

  # CPU fallback for mission workers.
  VISION_MODE=cpu bash "${REPO}/scripts/start_vision_workers.sh" start >>"$LOG" 2>&1 || true

  discover_compute
}

main() {
  with_lock
  log "mission watchdog tick"

  # Process-level resilience tick (local restart + peer handoff queue).
  bash "${REPO}/scripts/n8n-watchdog.sh" >>"$LOG" 2>&1 || true

  ensure_n8n
  ensure_forge
  ensure_runtime_corrections
  ensure_gpu_slots_from_heartbeat || true
  C2_UNRECOVERABLE_NODE=0
  ensure_c2_cards_reporting || true

  discover_compute
  recover_if_no_compute
  if [[ "$MISSION_DYNAMIC_ROUTE_UPDATE" == "1" ]]; then
    route_to_available || true
  elif [[ "$C2_REROUTE_ON_UNRECOVERABLE" == "1" && "$C2_UNRECOVERABLE_NODE" == "1" ]]; then
    log "unrecoverable node detected; forcing one-shot dynamic reroute"
    route_to_available || true
  else
    log "dynamic route update disabled; keeping Forge routing state"
  fi

  # Keep existing route/webhook checks for n8n + forge paths.
  bash "${REPO}/scripts/fleet-route-health.sh" >>"$LOG" 2>&1 || true

  log "mission watchdog done"
}

main "$@"
