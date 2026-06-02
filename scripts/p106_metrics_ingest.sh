#!/usr/bin/env bash
# P106 curator: trim audit/PAMP lines and optionally POST to nautivecs.
set -euo pipefail

METRICS="${HOME}/.cache/cesarops/audit_runs.jsonl"
NAUTIVECS="${NAUTIVECS_URL:-http://127.0.0.1:5003/query}"
MAX_CHARS=2000

mkdir -p "$(dirname "$METRICS")"
[[ -f "$METRICS" ]] || exit 0

tail -n 20 "$METRICS" | while read -r line; do
  trimmed=$(echo "$line" | head -c "$MAX_CHARS")
  if curl -sf --max-time 5 "$NAUTIVECS" >/dev/null 2>&1; then
    curl -sf -X POST "$NAUTIVECS" \
      -H 'Content-Type: application/json' \
      -d "{\"query\":\"fleet metrics $(echo "$trimmed" | head -c 200)\",\"top_k\":3}" \
      >/dev/null 2>&1 || true
  fi
done
echo "[p106-metrics] processed tail of $METRICS"
