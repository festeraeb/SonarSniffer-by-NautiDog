#!/usr/bin/env bash
# T440: fix rclone.conf and enable cesarops-backup for Nomad rclone-state-sync.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
CONF_DIR="${HOME}/.config/rclone"
CONF="${CONF_DIR}/rclone.conf"
BACKUP_ROOT="${CESAROPS_BACKUP_ROOT:-/codebase/backups/cesarops-state}"
EXAMPLE="${REPO}/infra/nomad/rclone/rclone.conf.example"

log() { echo "[rclone-setup] $*"; }

if [[ "$(hostname -s)" != "t440cesarops" && "$(hostname -s)" != *t440* ]]; then
  log "warning: expected T440 host; continuing anyway (hostname=$(hostname -s))"
fi

mkdir -p "$CONF_DIR" "$BACKUP_ROOT/state"
chmod 700 "$CONF_DIR"

if [[ -f "$CONF" ]]; then
  if ! rclone --config "$CONF" listremotes &>/dev/null; then
    stamp="$(date +%Y%m%dT%H%M%SZ)"
    log "backing up broken config → ${CONF}.broken.${stamp}"
    cp -a "$CONF" "${CONF}.broken.${stamp}"
    rm -f "$CONF"
  fi
fi

if [[ ! -f "$CONF" ]]; then
  if [[ ! -f "$EXAMPLE" ]]; then
    log "ERROR: missing $EXAMPLE"
    exit 1
  fi
  cp "$EXAMPLE" "$CONF"
  chmod 600 "$CONF"
  log "installed $CONF from example (alias → $BACKUP_ROOT)"
fi

export RCLONE_CONFIG="$CONF"
if ! rclone listremotes | grep -q '^cesarops-backup:'; then
  log "ERROR: cesarops-backup still missing after install"
  exit 1
fi

log "remotes:"
rclone listremotes

log "dry-run sync:"
bash "${REPO}/infra/nomad/rclone/rclone-state-sync.sh" || true

log "done — backup root: $BACKUP_ROOT"
log "Nomad: export NOMAD_ADDR=http://127.0.0.1:4646"
log "  nomad job run ${REPO}/infra/nomad/jobs/rclone-state-sync.nomad.hcl"
log "  nomad job periodic status rclone-state-sync"
log "  nomad job allocs -all rclone-state-sync"
