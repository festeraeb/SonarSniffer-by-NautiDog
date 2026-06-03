#!/usr/bin/env bash
# Ingest Straits calibration/heuristics into nautivecs (:5003) and optionally OpenMemory (:8765).
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
NAUTIVECS="${NAUTIVECS_URL:-http://127.0.0.1:5003}"
OPENMEMORY="${OPENMEMORY_URL:-http://127.0.0.1:8765}"
RECORDS="${RECORDS:-$REPO/data/forge/openmemory_straits_records.jsonl}"
BLUEPRINT_DOC="${BLUEPRINT_DOC:-$REPO/nautivecs/src/blueprints/satellite_physics.rs}"

log() { echo "[ingest-forge] $*"; }

post_nautivecs() {
  local text="$1" tags="$2" path="$3"
  curl -sf -X POST "${NAUTIVECS}/add" \
    -H 'Content-Type: application/json' \
    -d "$(jq -n --arg t "$text" --arg g "$tags" --arg p "$path" \
      '{text: $t, tags: $g, file_path: $p, source: "forge_runtime"}')" \
    >/dev/null && log "nautivecs + $(basename "$path")" || log "warn: nautivecs add failed for $path"
}

if ! curl -sf --connect-timeout 3 "${NAUTIVECS}/health" >/dev/null 2>&1; then
  log "nautivecs not up at ${NAUTIVECS} — start cesarops-nautivecs.service first"
  exit 1
fi

if [[ -f "$BLUEPRINT_DOC" ]]; then
  post_nautivecs "$(cat "$BLUEPRINT_DOC")" "nautivecs,blueprint,satellite_physics,f64" "$BLUEPRINT_DOC"
fi

while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  mem_type=$(echo "$line" | jq -r '.memory_type // "heuristic"')
  module=$(echo "$line" | jq -r '.target_module // .target_name // "straits"')
  post_nautivecs "$line" "openmemory,${mem_type},${module},straits" "data/forge/openmemory_straits_records.jsonl"
done < "$RECORDS"

if curl -sf --connect-timeout 2 "${OPENMEMORY}/" >/dev/null 2>&1; then
  log "OpenMemory reachable at ${OPENMEMORY} — mirror records via your MCP ingest if configured"
else
  log "OpenMemory offline; heuristics are in nautivecs + ${RECORDS}"
fi

log "done"
