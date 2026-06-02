#!/usr/bin/env bash
# Forge health probe for n8n / fleet-ops. Emits JSON on stdout.
#
# States:
#   healthy      — Forge up, not busy
#   busy_ok      — send_busy but recent lane progress and/or GPU activity
#   stalled      — send_busy, no progress long enough → interrupt recommended
#   broken       — process/health dead OR send_busy wedged past broken threshold
#
# Env:
#   FORGE_URL, LANE, STALL_SECS (default 1800), BROKEN_SECS (default 7200)
#   RECOVER=1  — run recommended recovery and include result in JSON
set -uo pipefail

_hn_probe="$(hostname -s | tr '[:upper:]' '[:lower:]')"
if [[ "$_hn_probe" == *t440* ]]; then
  FORGE_URL="${FORGE_URL:-http://10.0.0.201:9100}"
else
  FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
fi
LANE="${LANE:-lane-a}"
STALL_SECS="${FORGE_STALL_SECS:-${STALL_SECS:-1800}}"
BROKEN_SECS="${FORGE_BROKEN_SECS:-${BROKEN_SECS:-7200}}"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
[[ -d /mnt/t440/repo ]] && REPO="/mnt/t440/repo"
[[ -d /mnt/t440/codebase/repos/wreckhunter2000-1 ]] && REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"
OUT_JSON="${OUT_JSON:-/tmp/forge_health_last.json}"
FJ_STATUS="${REPO}/var/fleet-jobs/forge_health_last.json"

health_ok=0
status_ok=0
port_ok=0
systemd_active=0
coder_ok=0
thinker_ok=0

curl -sf --max-time 5 "${FORGE_URL}/health" >/dev/null 2>&1 && health_ok=1
curl -sf --max-time 5 "${FORGE_URL}/forge/status" >/dev/null 2>&1 && status_ok=1
if [[ "$_hn_probe" == *t440* ]]; then
  port_ok=1
  systemd_active=1
else
  ss -tln 2>/dev/null | grep -q ':9100 ' && port_ok=1
  systemctl is-active --quiet cesarops-forge-v2.service 2>/dev/null && systemd_active=1
fi

activity_json=$(curl -sf --max-time 5 "${FORGE_URL}/lanes/activity" 2>/dev/null || echo '{}')
status_json=$(curl -sf --max-time 5 "${FORGE_URL}/forge/status" 2>/dev/null || echo '{}')

curl -sf --max-time 5 "http://127.0.0.1:5001/health" >/dev/null 2>&1 && coder_ok=1
curl -sf --max-time 5 "http://127.0.0.1:5002/health" >/dev/null 2>&1 && thinker_ok=1

export ACTIVITY_JSON="$activity_json"
export STATUS_JSON="$status_json"
export PROBE_LANE="$LANE"
export PROBE_STALL_SECS="$STALL_SECS"
export PROBE_BROKEN_SECS="$BROKEN_SECS"
export PROBE_HEALTH_OK="$health_ok"
export PROBE_STATUS_OK="$status_ok"
export PROBE_PORT_OK="$port_ok"
export PROBE_SYSTEMD_OK="$systemd_active"
export PROBE_CODER_OK="$coder_ok"
export PROBE_THINKER_OK="$thinker_ok"

probe_json=$(python3 <<'PY'
import json, os, subprocess, time

activity = json.loads(os.environ.get("ACTIVITY_JSON") or "{}")
status = json.loads(os.environ.get("STATUS_JSON") or "{}")
lane = os.environ.get("PROBE_LANE", "lane-a")
stall_secs = int(os.environ.get("PROBE_STALL_SECS", "1800"))
broken_secs = int(os.environ.get("PROBE_BROKEN_SECS", "7200"))
health_ok = os.environ.get("PROBE_HEALTH_OK") == "1"
status_ok = os.environ.get("PROBE_STATUS_OK") == "1"
port_ok = os.environ.get("PROBE_PORT_OK") == "1"
systemd_active = os.environ.get("PROBE_SYSTEMD_OK") == "1"
coder_ok = os.environ.get("PROBE_CODER_OK") == "1"
thinker_ok = os.environ.get("PROBE_THINKER_OK") == "1"

now = int(time.time())
acts = [a for a in activity.get("activity", []) if a.get("lane") == lane]
last = acts[-1] if acts else {}
last_ts = int(last.get("ts") or 0)
last_kind = last.get("kind") or "none"
last_text = (last.get("text") or "")[:240]

non_user = [a for a in acts if a.get("kind") != "user"]
non_user_ts = int(non_user[-1].get("ts") or 0) if non_user else 0
progress_ts = non_user_ts or last_ts
progress_age = now - progress_ts if progress_ts else 999999

send_busy = bool(status.get("send_busy"))
if not status_ok:
    send_busy = False

gpu_active = False
try:
    out = subprocess.check_output(
        ["nvidia-smi", "--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"],
        text=True,
        timeout=8,
    )
    for line in out.splitlines():
        v = line.strip().split()[0] if line.strip() else "0"
        if v.isdigit() and int(v) > 8:
            gpu_active = True
            break
except Exception:
    pass

process_ok = port_ok and (health_ok or systemd_active)

if not process_ok:
    state = "broken"
    reason = "forge_unreachable_or_down"
    action = "restart_forge"
elif not send_busy:
    state = "healthy"
    reason = "idle"
    action = "none"
elif gpu_active or progress_age < stall_secs:
    state = "busy_ok"
    reason = "active_run"
    action = "none"
elif progress_age >= broken_secs:
    state = "broken"
    reason = f"send_busy_wedged_{progress_age}s"
    action = "full_recovery"
elif progress_age >= stall_secs:
    state = "stalled"
    reason = f"no_progress_{progress_age}s"
    action = "interrupt_and_clear"
else:
    state = "busy_ok"
    reason = "send_busy_recent"
    action = "none"

if state not in ("broken",) and (not coder_ok or not thinker_ok):
    if action == "none" and not send_busy:
        action = "restart_llama_p100"

out = {
    "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "state": state,
    "reason": reason,
    "recommended_action": action,
    "lane": lane,
    "send_busy": send_busy,
    "progress_age_sec": progress_age,
    "stall_secs": stall_secs,
    "broken_secs": broken_secs,
    "last_kind": last_kind,
    "last_text": last_text,
    "gpu_active": gpu_active,
    "checks": {
        "health_ok": bool(health_ok),
        "status_ok": bool(status_ok),
        "port_ok": bool(port_ok),
        "systemd_active": bool(systemd_active),
        "coder_ok": bool(coder_ok),
        "thinker_ok": bool(thinker_ok),
    },
    "healthy": state in ("healthy", "busy_ok"),
    "needs_recovery": state in ("stalled", "broken"),
}
print(json.dumps(out, indent=2))
PY
)

echo "$probe_json" | tee "$OUT_JSON" >/dev/null
mkdir -p "$(dirname "$FJ_STATUS")" 2>/dev/null || true
echo "$probe_json" >"$FJ_STATUS" 2>/dev/null || true

if [[ "${RECOVER:-0}" == "1" ]]; then
  action=$(echo "$probe_json" | python3 -c "import json,sys; print(json.load(sys.stdin).get('recommended_action','none'))")
  if [[ "$action" != "none" ]]; then
    bash "${REPO}/scripts/forge-health-recover.sh" "$action" || true
  fi
fi

state=$(echo "$probe_json" | python3 -c "import json,sys; print(json.load(sys.stdin).get('state','broken'))")
case "$state" in
  healthy|busy_ok) exit 0 ;;
  stalled) exit 1 ;;
  broken) exit 2 ;;
  *) exit 2 ;;
esac
