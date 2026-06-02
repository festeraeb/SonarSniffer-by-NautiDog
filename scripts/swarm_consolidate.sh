#!/usr/bin/env bash
# Swarm consolidation: gather straits download outputs onto the roomy /data
# partition (998 GB free) and OFF the tight /codebase partition (~50 GB free,
# 90% full). Two sources are folded in:
#
#   1. REMOTE (ML350e / cesarops2, 10.0.0.200): rsync each remote dir over the
#      LAN once its remote download finishes.
#   2. LOCAL (this T440): move each local data/straits_* dir into /data once its
#      local download finishes — frees /codebase immediately.
#
# A disk guard pauses new transfers if the target partition crosses a high-water
# mark, so we never fill /data. Resumable (rsync --partial) and safe to re-run.
#
# Usage: bash scripts/swarm_consolidate.sh [poll_seconds]
set -uo pipefail

REMOTE="${REMOTE:-cesarops@10.0.0.200}"
REMOTE_DATA="${REMOTE_DATA:-/codebase/repos/wreckhunter2000-1/data}"

# Canonical satellite-data home (roomy /data, NOT /codebase).
LOCAL_DATA="${LOCAL_DATA:-/data/cesarops/satellite_data/straits}"

# This T440's repo data dir (lives on /codebase — the partition we're draining).
T440_REPO_DATA="${T440_REPO_DATA:-/home/cesarops/wreckhunter2000-1/data}"

POLL="${1:-120}"

# Disk guard: skip/pause new transfers if the /data target crosses this % used.
DISK_GUARD_PCT="${DISK_GUARD_PCT:-90}"

mkdir -p "$LOCAL_DATA"

# Remote dirs the ML350e was tasked with (rsync-pulled over the LAN).
REMOTE_DIRS=(straits_optical_2023 straits_optical_2022)

# Local T440 dirs to fold in (moved off /codebase into /data).
LOCAL_DIRS=(straits_multisensor straits_optical_2022 straits_optical_2023 \
            straits_sar_slc straits_20day_stack straits_optical_clear)

ssh_opts=(-o BatchMode=yes -o StrictHostKeyChecking=no -o ConnectTimeout=8)

# ── helpers ───────────────────────────────────────────────────────────────────

# % used on the partition backing $LOCAL_DATA.
target_pct_used() {
  df --output=pcent "$LOCAL_DATA" 2>/dev/null | tail -1 | tr -dc '0-9'
}

disk_guard_ok() {
  local pct; pct="$(target_pct_used)"
  if [[ -n "$pct" && "$pct" -ge "$DISK_GUARD_PCT" ]]; then
    echo "[guard] target ${LOCAL_DATA} at ${pct}% (>=${DISK_GUARD_PCT}%) — pausing transfers" >&2
    return 1
  fi
  return 0
}

wait_for_disk() {
  # Block until the target drops below the guard threshold.
  until disk_guard_ok; do
    echo "[guard] $(date +%H:%M:%S) waiting for space under ${DISK_GUARD_PCT}% on $LOCAL_DATA; sleeping ${POLL}s" >&2
    sleep "$POLL"
  done
}

remote_downloads_active() {
  local n
  n="$(ssh "${ssh_opts[@]}" "$REMOTE" 'ps aux | grep "[u]niversal_downloader" | wc -l' 2>/dev/null || echo 0)"
  [[ "${n:-0}" -gt 0 ]]
}

# Is a local download still writing to data/<dir>? Match the --output arg.
# Use `ps -ww -eo args` (full, untruncated argv) — `ps aux` clips long lines to
# the terminal width and hides the trailing --output path.
local_download_active() {
  local dir="$1"
  local n
  n="$(ps -ww -eo args 2>/dev/null | grep "[u]niversal_downloader" \
       | grep -c -- "--output[ =][^ ]*${dir}\$\|--output[ =][^ ]*${dir}[ /]" 2>/dev/null || echo 0)"
  [[ "${n:-0}" -gt 0 ]]
}

# ── 1. LOCAL fold-in (move T440 dirs off /codebase as soon as each is idle) ────
fold_local() {
  for d in "${LOCAL_DIRS[@]}"; do
    local src="$T440_REPO_DATA/$d"
    [[ -d "$src" ]] || continue

    if local_download_active "$d"; then
      echo "[local] $d still downloading — defer"
      continue
    fi

    wait_for_disk
    local dst="$LOCAL_DATA/$d"
    echo "[local] moving $d -> $dst"
    # rsync then delete source: works across filesystems (/codebase -> /data),
    # --partial allows resume, --remove-source-files drains /codebase as it goes.
    if rsync -a --partial --remove-source-files --info=progress2 "$src/" "$dst/"; then
      # Clean up emptied source tree (rsync leaves dirs behind).
      find "$src" -type d -empty -delete 2>/dev/null
      rmdir "$src" 2>/dev/null
      echo "[local] $d folded into /data (source drained)"
    else
      echo "[local] $d move had errors (re-run to resume)"
    fi
  done
}

# ── 2. REMOTE pull (rsync ML350e dirs once remote downloads finish) ────────────
pull_remote() {
  echo "[remote] waiting for downloads on $REMOTE to finish (poll ${POLL}s)..."
  while remote_downloads_active; do
    echo "[remote] $(date +%H:%M:%S) remote still downloading; sleeping ${POLL}s"
    # Use the wait window to fold in any local dirs that have gone idle.
    fold_local
    sleep "$POLL"
  done
  echo "[remote] downloads idle — starting pull"

  for d in "${REMOTE_DIRS[@]}"; do
    wait_for_disk
    echo "[remote] rsync $d ..."
    rsync -az --partial --info=progress2 \
      "$REMOTE:$REMOTE_DATA/$d/" "$LOCAL_DATA/$d/" \
      && echo "[remote] $d done" \
      || echo "[remote] $d rsync had errors (re-run to resume)"
  done
}

# ── run ────────────────────────────────────────────────────────────────────────
echo "[consolidate] target=$LOCAL_DATA guard=${DISK_GUARD_PCT}% poll=${POLL}s"
echo "[consolidate] $(df -h "$LOCAL_DATA" | tail -1)"

# First pass at local fold-in (anything already idle moves now).
fold_local
# Then drain the remote (folding local dirs in during the wait window).
pull_remote
# Final local sweep for anything (e.g. straits_optical_clear) that finished late.
fold_local

echo "[consolidate] complete. /data sizes:"
du -sh "$LOCAL_DATA"/* 2>/dev/null
echo "[consolidate] $(df -h "$LOCAL_DATA" | tail -1)"
echo "[consolidate] $(df -h /codebase | tail -1)"
