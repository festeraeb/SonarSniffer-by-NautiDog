#!/usr/bin/env bash
# Overnight watchdog for CESAROPS Forge geo run.
# - Monitors lane-a for stalls / failures
# - Restarts llama stack and Forge if needed
# - Logs graded activity for morning review
# - Falls back to P106 (:5201) if MoE (:5200) dies
#
# Run:  nohup bash scripts/forge_overnight_watch.sh >>/data/cesarops/logs/overnight-watch.log 2>&1 &
set -uo pipefail

FORGE_URL="http://127.0.0.1:9100"
LANE="lane-a"
LOG="/data/cesarops/logs/overnight-watch.log"
GRADE_LOG="/data/cesarops/logs/forge-overnight-grades.md"
REPO="/codebase/repos/wreckhunter2000-1"
FORGE_BIN="/data/cargo-target/release/cesarops-forge-v2"
C2="10.0.0.201"

# Max seconds without a 'done' event before declaring stall
STALL_SECS=900   # 15 min
POLL=30          # check every 30s
RESEED_COOLDOWN_SECS=300

# Watchdog advisor model pool (priority order):
#   1) M2200 laptop tiny model
#   2) cesarops2 P1000/Picasso tiny model
#   3) cesarops2 P106 small R1/Qwen fallback
WATCHDOG_MODEL_POOL="${WATCHDOG_MODEL_POOL:-http://100.110.214.86:5571,http://10.0.0.201:5571,http://10.0.0.201:5201}"
# Full-time preferred watchdog endpoint. For today, use P106 (:5201).
# When P1000/Picasso is back in role, set:
#   WATCHDOG_PREFERRED_ENDPOINT=http://10.0.0.201:5571
WATCHDOG_PREFERRED_ENDPOINT="${WATCHDOG_PREFERRED_ENDPOINT:-http://10.0.0.201:5201}"

# The task to resume if lane-a is empty / cleared after a failure
RESUME_TASK="You are a GIS / remote sensing engineer building CESAROPS — a Great Lakes search-and-rescue and shipwreck hunting platform. Continue working on the current task list. Read the existing codebase first, then implement the next incomplete item. Write code using write_file. Do not stop until all items are complete or you hit an unresolvable blocker."

log() {
    local msg="[$(date '+%Y-%m-%d %H:%M:%S')] $*"
    echo "$msg"
    echo "$msg" >> "$LOG" 2>/dev/null || true
}
grade() { echo "$*" >> "$GRADE_LOG"; }

pick_watchdog_endpoint() {
    if [[ -n "${WATCHDOG_PREFERRED_ENDPOINT:-}" ]]; then
        if curl -sf --max-time 4 "${WATCHDOG_PREFERRED_ENDPOINT%/}/v1/models" >/dev/null 2>&1; then
            echo "$WATCHDOG_PREFERRED_ENDPOINT"
            return 0
        fi
    fi
    local IFS=','
    local ep
    for ep in $WATCHDOG_MODEL_POOL; do
        ep="${ep%"${ep##*[![:space:]]}"}"
        ep="${ep#"${ep%%[![:space:]]*}"}"
        [[ -n "$ep" ]] || continue
        if curl -sf --max-time 4 "${ep%/}/v1/models" >/dev/null 2>&1; then
            echo "$ep"
            return 0
        fi
    done
    return 1
}

ensure_forge_up() {
    curl -sf --max-time 5 "$FORGE_URL/health" >/dev/null 2>&1 && return 0
    log "Forge down — restarting via systemd"
    sudo systemctl restart cesarops-forge-v2 2>/dev/null || {
        pkill -f cesarops-forge-v2 2>/dev/null || true
        sleep 2
        setsid "$FORGE_BIN" >>"$LOG" 2>&1 &
    }
    for i in $(seq 1 20); do
        sleep 3
        curl -sf --max-time 5 "$FORGE_URL/health" >/dev/null 2>&1 && { log "Forge back up (${i}×3s)"; return 0; }
    done
    log "ERROR: Forge failed to start — check $FORGE_BIN"
    return 1
}

reapply_routing() {
    curl -sf -X POST "$FORGE_URL/cluster/routing/preset/cesarops-geo-fleet" \
        -H 'Content-Type: application/json' -d '{"start_workers":false}' >/dev/null 2>&1 || true
    log "routing preset reapplied: cesarops-geo-fleet"
}

ensure_llama_up() {
    local coder_ok=0 r1_ok=0 moe_ok=0
    curl -sf --max-time 5 "http://127.0.0.1:5001/v1/models" >/dev/null 2>&1 && coder_ok=1
    curl -sf --max-time 5 "http://127.0.0.1:5002/v1/models" >/dev/null 2>&1 && r1_ok=1
    curl -sf --max-time 5 "http://${C2}:5200/v1/models" >/dev/null 2>&1 && moe_ok=1

    if [[ $coder_ok == 0 || $r1_ok == 0 ]]; then
        log "T440 llama down (coder=$coder_ok r1=$r1_ok) — restarting"
        bash "$REPO/scripts/p100_gemma_r1_dual.sh" start >>"$LOG" 2>&1 || true
        sleep 30
    fi
    if [[ $moe_ok == 0 ]]; then
        log "c2 MoE :5200 down — switching corrector to p106 :5201"
        curl -sf -X POST "$FORGE_URL/cluster/routing" \
            -H 'Content-Type: application/json' \
            -d '{"reviewer_endpoint":"http://10.0.0.201:5201","corrector_endpoint":"http://10.0.0.201:5201"}' >/dev/null 2>&1 || true
    fi
}

resume_task() {
    log "Resuming lane-a task"
    curl -sf -X POST "$FORGE_URL/send" \
        -H 'Content-Type: application/json' \
        --max-time 30 \
        -d "{\"message\":$(python3 -c "import json,sys; print(json.dumps(sys.argv[1]))" "$RESUME_TASK"),\"lane\":\"lane-a\"}" \
        >/dev/null 2>&1 &
    log "Task resent (background)"
}

write_grade_header() {
    grade ""
    grade "## Session $(date '+%Y-%m-%d %H:%M')"
    grade "| Time | Lane | Event | Summary |"
    grade "|------|------|-------|---------|"
}

grade_activity() {
    local ts_cutoff=$1
    local activity_json
    activity_json=$(curl -sf --max-time 5 "$FORGE_URL/lanes/activity" 2>/dev/null || echo '{}')
    ACTIVITY_JSON="$activity_json" python3 -c "
import os, json, datetime
data = json.loads(os.environ.get('ACTIVITY_JSON', '{}'))
cutoff = int($ts_cutoff)
for a in data.get('activity', []):
    if a.get('ts', 0) < cutoff:
        continue
    t = datetime.datetime.fromtimestamp(a.get('ts', 0)).strftime('%H:%M:%S')
    lane = a.get('lane', '')
    kind = a.get('kind', '')
    text = (a.get('text') or '')[:120].replace('|', '/').replace('\\n', ' ')
    print(f'| {t} | {lane} | {kind} | {text} |')
"
}

# ── Main loop ─────────────────────────────────────────────────────────────────
mkdir -p "$(dirname "$LOG")" "$(dirname "$GRADE_LOG")"
log "=== overnight watchdog START (stall_secs=$STALL_SECS poll=${POLL}s) ==="
write_grade_header

LAST_DONE_TS=0
LAST_GRADE_AT=$(date +%s)
GRADE_INTERVAL=1800   # grade every 30 min
LAST_RESEED_AT=0
LAST_WATCHDOG_EP=""
LAST_SEEN_EVENT_TS=0

while true; do
    sleep "$POLL"

    ensure_forge_up || { sleep 30; continue; }
    ensure_llama_up

    # Keep a dedicated watchdog advisor endpoint selected in requested order.
    watchdog_ep=$(pick_watchdog_endpoint || true)
    if [[ -n "${watchdog_ep:-}" ]]; then
        if [[ "$watchdog_ep" != "$LAST_WATCHDOG_EP" ]]; then
            log "watchdog_role endpoint => $watchdog_ep (pool order: M2200 -> P1000 -> P106)"
            LAST_WATCHDOG_EP="$watchdog_ep"
        fi
    else
        log "watchdog_role endpoint => none healthy in pool: $WATCHDOG_MODEL_POOL"
    fi

    # Fetch latest activity
    activity=$(curl -sf --max-time 5 "$FORGE_URL/lanes/activity" 2>/dev/null || echo '{}')

    # Stream newly-seen lane events into watchdog logs so we monitor real output.
    event_dump=$(echo "$activity" | python3 -c "
import sys,json
d=json.load(sys.stdin)
acts=[a for a in d.get('activity',[]) if a.get('lane')=='lane-a']
last_seen=int('$LAST_SEEN_EVENT_TS')
new=[a for a in acts if int(a.get('ts',0))>last_seen]
mx=last_seen
for a in new:
    ts=int(a.get('ts',0))
    if ts>mx: mx=ts
print('__LAST_TS__',mx)
for a in new:
    kind=a.get('kind','')
    text=(a.get('text') or '').replace('\\n',' ')[:220]
    print(f'__EVENT__ {int(a.get(\"ts\",0))} {kind} | {text}')
" 2>/dev/null || echo "__LAST_TS__ $LAST_SEEN_EVENT_TS")
    while IFS= read -r line; do
        if [[ "$line" == __LAST_TS__* ]]; then
            LAST_SEEN_EVENT_TS=$(echo "$line" | awk '{print $2}')
        elif [[ "$line" == __EVENT__* ]]; then
            log "lane-live ${line#__EVENT__ }"
        fi
    done <<< "$event_dump"

    last_event=$(echo "$activity" | python3 -c "
import sys,json
d=json.load(sys.stdin)
acts=[a for a in d.get('activity',[]) if a.get('lane')=='lane-a']
if acts:
    a=acts[-1]
    print(a.get('ts',0), a.get('kind',''), len(a.get('text') or ''))
else:
    print(0, 'none', 0)
" 2>/dev/null || echo "0 none 0")
    last_non_user_ts=$(echo "$activity" | python3 -c "
import sys,json
d=json.load(sys.stdin)
acts=[a for a in d.get('activity',[]) if a.get('lane')=='lane-a' and a.get('kind')!='user']
print(acts[-1].get('ts',0) if acts else 0)
" 2>/dev/null || echo "0")

    last_ts=$(echo "$last_event" | awk '{print $1}')
    last_kind=$(echo "$last_event" | awk '{print $2}')
    now=$(date +%s)
    age=$((now - last_ts))
    if [[ "$last_non_user_ts" -gt 0 ]]; then
        non_user_age=$((now - last_non_user_ts))
    else
        # If we have no assistant/done events yet, fall back to the latest lane event age.
        non_user_age=$age
    fi

    # Track done events
    if [[ "$last_kind" == "done" && "$last_ts" -gt "$LAST_DONE_TS" ]]; then
        LAST_DONE_TS=$last_ts
        log "done event at ts=$last_ts — task completed a round"
    fi

    # Stall detection
    if [[ "$non_user_age" -gt "$STALL_SECS" && "$last_kind" != "none" ]]; then
        if [[ $((now - LAST_RESEED_AT)) -lt "$RESEED_COOLDOWN_SECS" ]]; then
            log "STALL candidate but cooldown active (last reseed $((now - LAST_RESEED_AT))s ago)"
        else
        log "STALL: no non-user progress for ${non_user_age}s (last_kind=$last_kind) — interrupting and resuming"
        curl -sf -X POST "$FORGE_URL/interrupt" >/dev/null 2>&1 || true
        sleep 5
        reapply_routing
        resume_task
        LAST_RESEED_AT=$now
        fi
    fi

    # No lane-a activity at all → seed initial task
    if [[ "$last_kind" == "none" ]]; then
        log "lane-a empty — seeding initial task"
        reapply_routing
        resume_task
        LAST_RESEED_AT=$now
    fi

    # Periodic grading
    if [[ $((now - LAST_GRADE_AT)) -gt $GRADE_INTERVAL ]]; then
        log "writing grade snapshot"
        grade ""
        grade "### Grade snapshot $(date '+%H:%M')"
        grade_activity "$((now - GRADE_INTERVAL))" >> "$GRADE_LOG"
        LAST_GRADE_AT=$now
    fi

    # GPU heartbeat every 5 min
    if [[ $((now % 300)) -lt "$POLL" ]]; then
        gpu_line=$(nvidia-smi --query-gpu=index,utilization.gpu,memory.used --format=csv,noheader 2>/dev/null | tr '\n' ' ')
        log "GPUs: $gpu_line | last_kind=$last_kind age=${age}s"
    fi
done
