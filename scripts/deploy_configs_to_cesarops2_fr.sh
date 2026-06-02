#!/usr/bin/env bash
# From T440: push cesarops2-fr configs over Tailscale SSH.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
REMOTE="${REMOTE:-cesarops@cesarops2-fr}"
# Or: REMOTE=cesarops@100.x.x.x

log() { echo "[deploy-c2-fr] $*"; }

log "remote=${REMOTE} repo=${REPO}"
ssh -o ConnectTimeout=15 "$REMOTE" "mkdir -p ~/codebase/repos && test -d ${REPO} || ln -sf /mnt/t440/codebase/repos/wreckhunter2000-1 ${REPO} 2>/dev/null || true"

rsync -avz --rsync-path="mkdir -p ${REPO}/config/cesarops2-fr && rsync" \
  "${REPO}/config/cesarops2-fr/" \
  "${REMOTE}:${REPO}/config/cesarops2-fr/"

rsync -avz "${REPO}/cesarops-node/cesarops-node-cesarops2.toml" \
  "${REMOTE}:${REPO}/config/cesarops2-fr/cesarops-node-legacy.toml" 2>/dev/null || true

ssh "$REMOTE" "sudo bash ${REPO}/scripts/setup_cesarops2_fr_tailscale.sh" || \
  ssh "$REMOTE" "bash ${REPO}/scripts/setup_cesarops2_fr_tailscale.sh"

FR_IP=$(ssh "$REMOTE" 'tailscale ip -4 2>/dev/null' || true)
if [[ -n "$FR_IP" ]]; then
  bash "${REPO}/scripts/patch_cluster_config_cesarops2_fr.sh" "$FR_IP"
  log "Forge cluster_config patched with ${FR_IP}"
fi

log "done — verify: tailscale status | grep cesarops2-fr"
