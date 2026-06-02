#!/usr/bin/env bash
# Switch Forge active routing: c2 edge base, T440 backup snapshot, or named preset.
#
# Usage:
#   bash scripts/forge-routing-switch.sh edge          # c2 NautiInferer home layout
#   bash scripts/forge-routing-switch.sh backup        # T440 conductor snapshot (way back)
#   bash scripts/forge-routing-switch.sh preset <id>   # e.g. cake-fleet, c2-edge-default
#   bash scripts/forge-routing-switch.sh status
#   bash scripts/forge-routing-switch.sh save-backup   # snapshot current → backup file
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/cesarops-forge-v2" ]] || REPO="/codebase/repos/wreckhunter2000-1"
FORGE_DIR="${FORGE_V2_DIR:-$REPO/cesarops-forge-v2}"
ROUTING_DIR="$FORGE_DIR/routing"
ACTIVE_RS="$FORGE_DIR/routing_state.json"
ACTIVE_MS="$FORGE_DIR/mode_state.json"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"

log() { echo "[forge-routing] $*"; }

copy_active() {
  local rs_src=$1 ms_src=${2:-}
  cp -a "$rs_src" "$ACTIVE_RS"
  log "routing_state ← $(basename "$rs_src")"
  if [[ -n "$ms_src" && -f "$ms_src" ]]; then
    cp -a "$ms_src" "$ACTIVE_MS"
    log "mode_state ← $(basename "$ms_src")"
  fi
}

apply_preset_api() {
  local id=$1
  if curl -sf --max-time 10 -X POST "${FORGE_URL}/cluster/routing/preset/${id}" >/dev/null; then
    log "preset applied via Forge API: $id"
    return 0
  fi
  log "Forge API preset failed — applying via Python fallback"
  FORGE_V2_DIR="$FORGE_DIR" python3 - <<PY
import sys
sys.path.insert(0, "$REPO/scripts/integrate")
# minimal inline: call routing module if forge built, else manual copy from toml
import json, subprocess, os
preset_id = "$id"
repo = "$REPO"
# PRIMARY Forge is cesarops2 only — never apply presets to deprecated T440 :9100
urls = [os.environ.get("FORGE_URL", "http://127.0.0.1:9100")]
for url in urls:
    import urllib.request
    try:
        req = urllib.request.Request(f"{url}/cluster/routing/preset/{preset_id}", method="POST", data=b"")
        urllib.request.urlopen(req, timeout=15)
        print(f"ok via {url}")
        sys.exit(0)
    except Exception as e:
        print(f"skip {url}: {e}")
sys.exit(1)
PY
}

cmd_status() {
  log "FORGE_DIR=$FORGE_DIR"
  log "FORGE_URL=$FORGE_URL"
  if [[ -f "$ACTIVE_RS" ]]; then
    echo "--- routing_state.json ---"
    cat "$ACTIVE_RS"
  else
    log "no active routing_state.json"
  fi
  if [[ -f "$ACTIVE_MS" ]]; then
    echo "--- mode_state.json ---"
    cat "$ACTIVE_MS"
  fi
  curl -sf --max-time 3 "${FORGE_URL}/health" >/dev/null && log "Forge: UP" || log "Forge: DOWN"
}

cmd_save_backup() {
  mkdir -p "$ROUTING_DIR"
  cp -a "$ACTIVE_RS" "$ROUTING_DIR/routing_state.backup.t440.json"
  [[ -f "$ACTIVE_MS" ]] && cp -a "$ACTIVE_MS" "$ROUTING_DIR/mode_state.backup.t440.json" || true
  log "saved snapshot → $ROUTING_DIR/routing_state.backup.t440.json"
}

case "${1:-status}" in
  edge|base|c2)
    copy_active "$ROUTING_DIR/routing_state.edge.json" "$ROUTING_DIR/mode_state.edge.json"
  ;;
  backup|t440|restore)
    if [[ ! -f "$ROUTING_DIR/routing_state.backup.t440.json" ]]; then
      log "ERROR: missing $ROUTING_DIR/routing_state.backup.t440.json (run save-backup on T440 first)"
      exit 1
    fi
    copy_active "$ROUTING_DIR/routing_state.backup.t440.json" "$ROUTING_DIR/mode_state.backup.t440.json"
  ;;
  preset)
    pid="${2:?preset id}"
    apply_preset_api "$pid" || {
      log "ERROR: could not apply preset $pid"
      exit 1
    }
  ;;
  save-backup)
    cmd_save_backup
  ;;
  status)
    cmd_status
  ;;
  *)
    echo "Usage: $0 {edge|backup|preset <id>|save-backup|status}"
    exit 1
  ;;
esac

cmd_status
