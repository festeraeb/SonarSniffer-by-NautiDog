#!/usr/bin/env bash
# Fleet-wide file catalog: all nodes → merge → consolidation plan → vector ingest.
#
#   bash scripts/fleet_file_catalog.sh catalog [node]     # local node catalog
#   bash scripts/fleet_file_catalog.sh catalog-all        # every node in config (local + ssh)
#   bash scripts/fleet_file_catalog.sh merge
#   bash scripts/fleet_file_catalog.sh plan [--embed]     # optional P106 Jina
#   bash scripts/fleet_file_catalog.sh rs-deep-scan       # parallel GDAL deep (after --defer-rs-deep catalog)
#   bash scripts/fleet_file_catalog.sh all                # fast catalog → deep scan → merge → plan
#
set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/fleet_resolve.sh
source "${_SCRIPT_DIR}/lib/fleet_resolve.sh"
# shellcheck source=lib/fleet_ssh.sh
source "${_SCRIPT_DIR}/lib/fleet_ssh.sh"
export REPO
PY="${PYTHON:-python3}"
CATALOG_DIR="${FLEET_CATALOG_DIR}"
CONFIG="$REPO/config/fleet_catalog_roots.json"

log() { echo "[fleet-catalog] $*"; }

detect_node() {
  local h
  h="$(hostname -s | tr '[:upper:]' '[:lower:]')"
  if [[ "$h" == *t440* ]]; then echo t440
  elif [[ "$h" == *nautik* ]]; then echo nautik9
  else echo cesarops2; fi
}

cmd_catalog() {
  local node="${1:-$(detect_node)}"
  log "catalog local node=$node"
  local extra=(--defer-rs-deep)
  if [[ "${FLEET_CATALOG_INLINE_RS:-}" == "1" ]]; then
    extra=()
  fi
  "$PY" "$REPO/scripts/fleet_file_catalog.py" --node "$node" "${extra[@]}" "$@"
}

cmd_catalog_remote() {
  local node="$1"
  shift
  local ssh_target
  ssh_target="$("$PY" -c "
import json
from pathlib import Path
c=json.loads(Path('$CONFIG').read_text())
n=c['nodes'].get('$node',{})
print(n.get('ssh') or '')
")"
  if [[ -z "$ssh_target" ]]; then
    if [[ "$(detect_node)" == "$node" ]]; then
      cmd_catalog "$node" "$@"
      return
    fi
    log "no ssh for $node and not local — skip"
    return 0
  fi
  local remote_repo resolved
  remote_repo="$(fleet_ssh_remote_repo "$node")"
  resolved="$(fleet_ssh_resolve_target "$ssh_target")"
  log "catalog remote node=$node via ssh $resolved repo=$remote_repo"
  local extra=(--defer-rs-deep)
  [[ "${FLEET_CATALOG_INLINE_RS:-}" == "1" ]] && extra=()
  fleet_ssh "$ssh_target" \
    "REPO='$remote_repo' $PY '$remote_repo/scripts/fleet_file_catalog.py' --node '$node' ${extra[*]} $*"
  fleet_scp "${resolved}:${remote_repo}/var/fleet-catalog/${node}.jsonl" \
    "$CATALOG_DIR/${node}.jsonl" 2>/dev/null || log "warn: scp ${node}.jsonl failed"
}

cmd_catalog_all() {
  local nodes
  nodes="$("$PY" -c "
import json
from pathlib import Path
c = json.loads(Path('$CONFIG').read_text())
for nid, n in c.get('nodes', {}).items():
    if n.get('catalog_enabled', True):
        print(nid)
")"
  mkdir -p "$CATALOG_DIR"
  for n in $nodes; do
    cmd_catalog_remote "$n" "$@" || log "warn: catalog $n failed"
  done
}

cmd_rs_deep() {
  log "parallel RS deep scan (fleet workers — GDAL, not llama)"
  "$PY" "$REPO/scripts/fleet_rs_deep_scan.py" "$@"
}

cmd_merge() {
  "$PY" "$REPO/scripts/fleet_file_catalog_merge.py"
}

cmd_plan() {
  if [[ "${1:-}" == "--embed" ]]; then
    shift
    export RECOVERY_CUDA_DEVICE="${RECOVERY_CUDA_DEVICE:-0}"
    export CUDA_VISIBLE_DEVICES="${CUDA_VISIBLE_DEVICES:-$RECOVERY_CUDA_DEVICE}"
    bash "$REPO/scripts/p106_recovery_run.sh" bootstrap 2>/dev/null || true
    VENV="${RECOVERY_VENV:-$HOME/.venvs/cesarops-recovery}"
    if [[ -x "$VENV/bin/python" ]]; then
      "$VENV/bin/python" "$REPO/scripts/fleet_catalog_plan.py" --embed "$@"
    else
      log "venv missing — plan without embed (rules only)"
      "$PY" "$REPO/scripts/fleet_catalog_plan.py" "$@"
    fi
  else
    "$PY" "$REPO/scripts/fleet_catalog_plan.py" "$@"
  fi
}

cmd_rs_summarize_start() {
  bash "$REPO/scripts/fleet_rs_llm_summarize.sh" start
}

cmd_all() {
  cmd_catalog_all "$@"
  cmd_merge
  cmd_rs_deep "$@"
  cmd_merge
  cmd_rs_summarize_start
  cmd_plan "$@"
  log "done — see $CATALOG_DIR/ (rs-llm summarize running in background)"
}

case "${1:-status}" in
  catalog)
    shift
    cmd_catalog "$@"
    ;;
  catalog-all) shift; cmd_catalog_all "$@" ;;
  merge) cmd_merge ;;
  rs-deep-scan|rs-deep) shift; cmd_rs_deep "$@" ;;
  rs-summarize-start) cmd_rs_summarize_start ;;
  plan) shift; cmd_plan "$@" ;;
  all) shift; cmd_all "$@" ;;
  pipeline-inventory|pipelines)
    shift
    log "pipeline/script inventory (stub|partial|complete, dupes) — no GeoTIFF"
    "$PY" "$REPO/scripts/fleet_pipeline_inventory.py" "$@"
    ;;
  rs-recovery|rs-pipeline-recovery)
    shift
    log "deep RS pipeline recovery (satellite|mag|bag) — repo vs laptopdump"
    "$PY" "$REPO/scripts/fleet_rs_pipeline_recovery.py" "$@"
    ;;
  inventory)
    shift
    log "full fleet inventory: catalog-all → merge → rs-deep → merge → plan --embed"
    cmd_catalog_all "$@"
    cmd_merge
    cmd_rs_deep "$@"
    cmd_merge
    cmd_rs_summarize_start
    cmd_plan --embed "$@"
    log "inventory complete — $CATALOG_DIR"
    ;;
  status)
    ls -la "$CATALOG_DIR" 2>/dev/null || echo "no catalog yet"
    ;;
  *)
    echo "Usage: $0 {catalog|catalog-all|merge|rs-deep-scan|plan|pipeline-inventory|all|inventory|status} [--max-files N]"
    exit 1
    ;;
esac
