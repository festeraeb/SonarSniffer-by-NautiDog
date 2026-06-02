#!/usr/bin/env bash
# Watch an active Forge lane run; interrupt if it stalls (no new activity / idle GPU too long).
set -euo pipefail
FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
LANE="${LANE:-lane-a}"
STALL_SECS="${STALL_SECS:-600}"
POLL="${POLL:-20}"
LAST_TS=0
IDLE_POLLS=0

log() { echo "[forge-watch] $*"; }

while true; do
  act=$(curl -sf --max-time 5 "$FORGE/lanes/activity" 2>/dev/null || echo '{}')
  ts=$(echo "$act" | python3 -c "
import sys,json
lane=sys.argv[1]
acts=[a for a in json.load(sys.stdin).get('activity',[]) if a.get('lane')==lane]
print(acts[-1]['ts'] if acts else 0)
" "$LANE" 2>/dev/null || echo 0)
  kind=$(echo "$act" | python3 -c "
import sys,json
lane=sys.argv[1]
acts=[a for a in json.load(sys.stdin).get('activity',[]) if a.get('lane')==lane]
print(acts[-1].get('kind','') if acts else '')
" "$LANE" 2>/dev/null || echo "")
  gpu1=$(nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader,id=1 2>/dev/null | head -1 | tr -d ' %')
  now=$(date +%s)
  age=$((now - ts))
  log "lane=$LANE last=$kind age=${age}s gpu1=${gpu1:-?}%"
  if [[ "$kind" == "done" ]]; then
    log "run finished"
    exit 0
  fi
  if [[ "$gpu1" =~ ^[0-9]+$ ]] && [[ "$gpu1" -lt 5 ]]; then
    IDLE_POLLS=$((IDLE_POLLS + 1))
  else
    IDLE_POLLS=0
  fi
  if [[ "$age" -ge "$STALL_SECS" ]] || [[ "$IDLE_POLLS" -ge 6 ]]; then
    log "stall detected — POST /interrupt"
    curl -sf -X POST "$FORGE/interrupt" || true
    exit 2
  fi
  sleep "$POLL"
done
