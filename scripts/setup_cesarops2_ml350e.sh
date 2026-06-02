#!/usr/bin/env bash
# ML350e Gen8 @ 10.0.0.201 — restore original cesarops2 config + Tailscale hostname cesarops2.
# Run ON cesarops2: bash scripts/setup_cesarops2_ml350e.sh
# Or from T440: ssh cesarops@10.0.0.201 'bash -s' < scripts/setup_cesarops2_ml350e.sh
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
[[ -d /mnt/t440/codebase/repos/wreckhunter2000-1 ]] && REPO=/mnt/t440/codebase/repos/wreckhunter2000-1

log() { echo "[cesarops2-ml350e] $*"; }

log "hostname=$(hostname) — configuring as cesarops2 (ML350e)"

# --- Tailscale (hostname cesarops2, NOT cesarops2-fr) ---
if ! command -v tailscale >/dev/null 2>&1; then
  log "Installing Tailscale…"
  curl -fsSL https://tailscale.com/install.sh | sh
fi
sudo systemctl enable --now tailscaled 2>/dev/null || true

if ! tailscale status >/dev/null 2>&1; then
  if [[ -z "${TS_AUTHKEY:-}" && -f "${REPO}/scripts/credentials.sh" ]]; then
    # shellcheck disable=SC1090
    source "${REPO}/scripts/credentials.sh"
  fi
  if [[ -n "${TS_AUTHKEY:-}" ]]; then
    sudo tailscale up --hostname=cesarops2 --ssh --authkey="$TS_AUTHKEY"
  else
    log "Join tailnet: sudo tailscale up --hostname=cesarops2 --ssh"
    log "Or: TS_AUTHKEY=tskey-auth-... sudo -E tailscale up --hostname=cesarops2 --ssh"
  fi
fi
TS_IP=$(tailscale ip -4 2>/dev/null || true)
log "Tailscale IP: ${TS_IP:-not connected yet}"

# --- /etc/cesarops (original cesarops2 node config) ---
sudo mkdir -p /etc/cesarops
sudo cp "${REPO}/cesarops-node/cesarops-node-cesarops2.toml" /etc/cesarops/cesarops-node.toml
sudo tee /etc/cesarops/cesarops2.env >/dev/null <<EOF
NODE_NAME=cesarops2
CESAROPS2_LAN_IP=10.0.0.201
CESAROPS2_LAN_ALT=10.0.0.200
T440_LAN_IP=10.0.0.61
FORGE_URL=http://10.0.0.61:9100
REPO=${REPO}
MODELS=/mnt/t440/models
CESAROPS2_HOST=10.0.0.201
CESAROPS2_TS_IP=${TS_IP}
EOF
sudo chmod 644 /etc/cesarops/cesarops2.env

# NFS models (if not mounted)
if ! mountpoint -q /mnt/t440/models 2>/dev/null; then
  log "Mount T440 models: sudo mount -t nfs 10.0.0.61:/codebase/models /mnt/t440/models"
fi

log "Installed /etc/cesarops/cesarops-node.toml + cesarops2.env"
log "Start lab: bash ${REPO}/scripts/cesarops2_research_lab.sh start"
log "Cake c2 workers: SKIP_P106=1 bash ${REPO}/scripts/cake/prep-c2-hetero-workers.sh"
