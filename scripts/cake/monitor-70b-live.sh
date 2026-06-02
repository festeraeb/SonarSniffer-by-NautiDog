#!/usr/bin/env bash
# Live TFLOPS + Cake log tail during 70B runs. Writes to MONITOR_LOG and stdout.
set -uo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
CAKE_LOG="${CAKE_LOG:-$HOME/.cache/cesarops/cake_fleet.log}"
MONITOR_LOG="${MONITOR_LOG:-/tmp/cake-70b-monitor.log}"
CESAROPS2_HOST="${CESAROPS2_HOST:-10.0.0.201}"
INTERVAL="${INTERVAL:-2}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"

peak_tflops() {
  case "$1" in
    *P100*) echo 18.7 ;;
    *2060*) echo 44 ;;
    *1070*) echo 19.3 ;;
    *P106*) echo 3.8 ;;
    *) echo "" ;;
  esac
}

gpu_block() {
  local host="$1" cmd="$2"
  echo "── ${host} $(date -u +%H:%M:%S) ──"
  while IFS=, read -r idx name util memu memt temp power; do
    [[ "$idx" == "index" ]] && continue
    peak=$(peak_tflops "$name")
    eff=""
    if [[ -n "$peak" && "$util" =~ ^[0-9]+$ ]]; then
      eff=$(awk -v p="$peak" -v u="$util" 'BEGIN{printf "%.1f", p*u/100}')
    fi
    printf "  GPU%s %-22s util=%3s%% mem=%s/%s MiB %s°C %sW eff≈%s TFLOPS\n" \
      "$idx" "$name" "$util" "$memu" "$memt" "$temp" "$power" "${eff:-idle}"
  done < <($cmd 2>/dev/null || true)
}

mkdir -p "$(dirname "$MONITOR_LOG")"
: >"$MONITOR_LOG"
echo "[monitor] logging to $MONITOR_LOG (interval ${INTERVAL}s)" | tee -a "$MONITOR_LOG"

tail -n 0 -F "$CAKE_LOG" 2>/dev/null | while IFS= read -r line; do
  echo "[cake] $line" | tee -a "$MONITOR_LOG"
done &
TAIL_PID=$!

trap 'kill $TAIL_PID 2>/dev/null; exit' INT TERM

while true; do
  {
    gpu_block "T440" "nvidia-smi --query-gpu=index,name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw --format=csv,noheader,nounits"
    gpu_block "cesarops2" "ssh -o ConnectTimeout=3 -o BatchMode=yes cesarops@${CESAROPS2_HOST} nvidia-smi --query-gpu=index,name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw --format=csv,noheader,nounits"
    if curl -sf --max-time 2 "${FORGE_URL}/monitor" >/tmp/forge-mon.json 2>/dev/null; then
      python3 -c "
import json
d=json.load(open('/tmp/forge-mon.json'))
for g in d.get('gpus',[]):
    print(f\"  [forge] {g.get('host')} {g.get('name')} {g.get('utilization_pct')}% {g.get('temperature_c')}°C\")
" 2>/dev/null || true
    fi
    if curl -sf --max-time 2 http://127.0.0.1:8081/v1/models >/dev/null 2>&1; then
      echo "  [cake-api] :8081 UP"
    else
      echo "  [cake-api] :8081 down"
    fi
    echo ""
  } | tee -a "$MONITOR_LOG"
  sleep "$INTERVAL"
done
