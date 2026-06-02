#!/usr/bin/env bash
# Bidirectional mirror + lightweight snapshots for Forge/tooling state.
# This is an operational mirror/backup layer (not block-level RAID).
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
LOG="${FLEET_MIRROR_LOG:-/data/cesarops/logs/fleet-dual-mirror.log}"
SNAP_ROOT="${FLEET_SNAPSHOT_ROOT:-/data/cesarops/snapshots/fleet-mirror}"
KEEP_DAYS="${FLEET_SNAPSHOT_KEEP_DAYS:-14}"

SYNC_PATHS=(
  "cesarops-forge-v2"
  "scripts"
  "missions"
  "systemd"
  "cesarops-detection"
  "cesarops-mcp-worker"
  "cesarops-satellite"
  "n8n_moe_tool_router.json"
  "n8n_fleet_ops_dispatch.json"
)

mkdir -p "$(dirname "$LOG")" "$SNAP_ROOT"

ts() { date -Iseconds; }
log() { echo "$(ts) [fleet-dual-mirror] $*" | tee -a "$LOG"; }

find_peer_repo() {
  local candidates=(
    "/mnt/t440/codebase/repos/wreckhunter2000-1"
    "/mnt/t440/repo"
    "/mnt/cesarops2/codebase/repos/wreckhunter2000-1"
    "/mnt/peer/codebase/repos/wreckhunter2000-1"
  )
  local c
  for c in "${candidates[@]}"; do
    if [[ -d "$c" && "$c" != "$REPO" ]]; then
      echo "$c"
      return 0
    fi
  done
  return 1
}

sync_path() {
  local src_root="$1"
  local dst_root="$2"
  local rel="$3"
  local src="${src_root%/}/$rel"
  local dst="${dst_root%/}/$rel"

  [[ -e "$src" ]] || return 0
  mkdir -p "$(dirname "$dst")"

  if [[ -d "$src" ]]; then
    rsync -a --update \
      --exclude '.git/' \
      --exclude 'target/' \
      --exclude '__pycache__/' \
      --exclude '.cursor/' \
      "$src/" "$dst/"
  else
    rsync -a --update "$src" "$dst"
  fi
}

mirror_repo_bidirectional() {
  local peer="$1"
  local p
  for p in "${SYNC_PATHS[@]}"; do
    sync_path "$REPO" "$peer" "$p"
  done
  for p in "${SYNC_PATHS[@]}"; do
    sync_path "$peer" "$REPO" "$p"
  done
}

sync_db_file() {
  local a="$1"
  local b="$2"
  [[ -f "$a" || -f "$b" ]] || return 0

  local winner
  if [[ -f "$a" && -f "$b" ]]; then
    if [[ "$a" -nt "$b" ]]; then winner="$a"; else winner="$b"; fi
  elif [[ -f "$a" ]]; then
    winner="$a"
  else
    winner="$b"
  fi

  if [[ "$winner" == "$a" ]]; then
    mkdir -p "$(dirname "$b")"
    rsync -a --update "$a" "$b"
  else
    mkdir -p "$(dirname "$a")"
    rsync -a --update "$b" "$a"
  fi
}

snapshot_now() {
  local stamp day_dir
  stamp="$(date +%Y%m%dT%H%M%SZ)"
  day_dir="${SNAP_ROOT}/$(date +%Y%m%d)"
  mkdir -p "$day_dir"

  tar -C "$REPO" -czf "$day_dir/forge-tooling-${stamp}.tgz" \
    cesarops-forge-v2/cluster_config.toml \
    cesarops-forge-v2/routing_state.json \
    scripts/n8n_activate_fleet_workflows.sh \
    scripts/import_n8n_health_workflows.sh \
    scripts/mission_service_watchdog.sh \
    scripts/fleet-route-health.sh \
    missions/n8n_fleet_ops_dispatch.json \
    missions/n8n_fleet_route_health.json \
    missions/n8n_forge_health.json \
    missions/n8n_prompt_tuner_worker.json \
    missions/n8n_predictive_async_moe.json \
    n8n_moe_tool_router.json \
    >/dev/null 2>&1 || true

  find "$SNAP_ROOT" -mindepth 1 -maxdepth 1 -type d -mtime "+${KEEP_DAYS}" -exec rm -rf {} + 2>/dev/null || true
}

main() {
  if [[ ! -d "$REPO" ]]; then
    log "repo not found: $REPO"
    exit 1
  fi

  local peer_repo
  if ! peer_repo="$(find_peer_repo)"; then
    log "peer repo mount not found; skipping mirror"
    snapshot_now
    exit 0
  fi

  log "mirroring repo between $REPO and $peer_repo"
  mirror_repo_bidirectional "$peer_repo"

  sync_db_file "/home/cesarops/.n8n/database.sqlite" "/mnt/t440/data/n8n_data/database.sqlite"
  sync_db_file "/data/n8n_data/database.sqlite" "/mnt/t440/data/n8n_data/database.sqlite"

  snapshot_now
  log "mirror complete"
}

main "$@"
