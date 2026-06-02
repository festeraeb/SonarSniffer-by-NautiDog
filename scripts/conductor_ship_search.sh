#!/usr/bin/env bash
# CESAROPS ship/wreck search — conductor tool battery (no LLM orchestration).
# Exercises forge /tool/*, pipeline CLIs, and ground-truth cross-checks.
set -euo pipefail

FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
REPO="/codebase/repos/wreckhunter2000-1"
# Full forge binary (orchestrator + /tool/*) — avoid stale package target/ copy:
FORGE_BIN="${FORGE_BIN:-/tmp/forge-install/bin/cesarops-forge-v2}"
REPORT="${REPORT:-/tmp/conductor_ship_search_$(date +%Y%m%d_%H%M%S).md}"
KNOWN_JSON="$REPO/backup/deploy/tools/cesarops-core-github/known_wrecks.json"
ERIE_CSV="/tmp/erie_known_wrecks_all.csv"

log() { echo "$*" | tee -a "$REPORT"; }
section() { log ""; log "## $1"; log ""; }

forge_tool() {
  local name="$1" args="$2"
  local out
  out=$(curl -sf -X POST "$FORGE/tool/$name" \
    -H 'Content-Type: application/json' \
    -d "{\"arguments\":$args}" 2>&1) || { log "**$name** — curl failed"; return 1; }
  python3 -c "
import json,sys
d=json.loads(sys.argv[1])
r=d.get('result',d)
print(r if isinstance(r,str) else json.dumps(d,indent=2))
" "$out" 2>/dev/null | head -c 2000 | tee -a "$REPORT"
  log ""
}

mkdir -p "$(dirname "$REPORT")"
: > "$REPORT"
log "# CESAROPS Great Lakes ship-search conductor report"
log "Generated: $(date -Is)"
log "Forge: $FORGE"
log "Repo: $REPO"

section "0. Prerequisites"
for p in "$FORGE/health" "$REPO/pipelines/mag/forge_cli.py"; do
  if curl -sf "$p" >/dev/null 2>&1 || [[ -f "$p" ]]; then
    log "- OK: $p"
  else
    log "- MISSING: $p"
  fi
done

section "1. Ground truth — known wrecks (Michigan / Huron)"
python3 <<'PY' | tee -a "$REPORT"
import json
from pathlib import Path
p = Path("/codebase/repos/wreckhunter2000-1/backup/deploy/tools/cesarops-core-github/known_wrecks.json")
w = json.loads(p.read_text())
samples = ["Andaste", "Gilcher", "Parnell"]
for name in samples:
    hit = next((v for v in w.values() if v.get("name") == name), None)
    if hit:
        lat = (hit["lat_min"] + hit["lat_max"]) / 2
        lon = (hit["lon_min"] + hit["lon_max"]) / 2
        print(f"- **{name}**: {lat:.4f}, {lon:.4f} ({hit.get('type')}, {hit.get('depth_ft')}ft)")
print(f"\nTotal catalog entries: {len(w)}")
PY

section "2. Erie known-wrecks DB (mag training targets)"
if python3 "$REPO/pipelines/mag/erie_known_wrecks_db.py" --output "$ERIE_CSV" >>"$REPORT" 2>&1; then
  log "Built $ERIE_CSV"
  wc -l "$ERIE_CSV" | tee -a "$REPORT"
else
  log "erie_known_wrecks_db.py failed"
fi

section "3. Forge tools — Lake Michigan bbox (41.8,-87.2,42.2,-86.8)"
BBOX='"bbox":"41.8,-87.2,42.2,-86.8"'
log "### detection_health"
forge_tool detection_health '{}' || true
log "### weather_window"
forge_tool weather_window "{${BBOX},\"check\":\"post_storm\",\"days\":14}" || true
log "### detection_scan (smoke — empty tiles)"
forge_tool detection_scan '{"region":"lake_michigan_wreck_scan","tiles":[]}' || true
log "### magnetic_dipole_detect (synthetic worker)"
forge_tool magnetic_dipole_detect '{"grid_path":"/tmp/forge_mag_grid.csv","pixel_size_m":25}' || true

section "4. Orchestrator plan only (no execute)"
PLAN=$(curl -sf -X POST "$FORGE/orchestrator/plan" \
  -H 'Content-Type: application/json' \
  -d '{"raw_text":"wreck hunt freighter south Lake Michigan","bbox":[41.8,-87.2,42.2,-86.8],"days_back":14}' 2>&1) || PLAN="curl failed"
echo "$PLAN" | python3 -m json.tool 2>/dev/null | head -80 | tee -a "$REPORT" || echo "$PLAN" | tee -a "$REPORT"

section "5. Pipeline CLIs (direct)"
log "### mag list"
python3 "$REPO/pipelines/mag/forge_cli.py" list 2>&1 | head -20 | tee -a "$REPORT"
log "### satellite list"
python3 "$REPO/pipelines/satellite/forge_cli.py" list 2>&1 | head -20 | tee -a "$REPORT"

section "6. Known vs unknown discriminator (Rust unit logic via worker)"
/home/cesarops/wreckhunter2000-1/target/release/cesarops-aeromagnetic-worker 2>&1 | tee -a "$REPORT" || true

section "7. Synthetic unknown probe (off-catalog coords)"
python3 <<'PY' | tee -a "$REPORT"
# Point in open water — should not match Andaste/Gilcher catalog centers (<500m)
import json, math
from pathlib import Path
w = json.loads(Path("/codebase/repos/wreckhunter2000-1/backup/deploy/tools/cesarops-core-github/known_wrecks.json").read_text())
def haversine(lat1, lon1, lat2, lon2):
    R = 6371000
    from math import radians, sin, cos, sqrt, atan2
    p1, p2 = radians(lat1), radians(lat2)
    dlat, dlon = radians(lat2-lat1), radians(lon2-lon1)
    a = sin(dlat/2)**2 + cos(p1)*cos(p2)*sin(dlon/2)**2
    return R * 2 * atan2(sqrt(a), sqrt(1-a))
probe = (43.0, -87.5)  # mid-lake unknown
dists = []
for v in w.values():
    lat = (v["lat_min"]+v["lat_max"])/2
    lon = (v["lon_min"]+v["lon_max"])/2
    dists.append((haversine(probe[0], probe[1], lat, lon), v["name"]))
dists.sort()
nearest = dists[0]
print(f"Probe {probe}: nearest catalog wreck **{nearest[1]}** at {nearest[0]/1000:.1f} km")
print("Label: UNKNOWN at probe" if nearest[0] > 2000 else "Label: near known wreck")
PY

log ""
log "---"
log "Report saved: $REPORT"
log "Next: start detection (\`cesarops-detection/scripts/start.sh\`), rerun detection_health, then orchestrator/execute."
