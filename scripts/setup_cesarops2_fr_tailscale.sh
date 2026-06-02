#!/usr/bin/env bash
# Run ON the new cesarops2-fr host (France). Installs Tailscale + drops legacy cesarops2 configs.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
CFG="${REPO}/config/cesarops2-fr"
TS_HOSTNAME="${TS_HOSTNAME:-cesarops2-fr}"
T440_TS_IP="${T440_TS_IP:-100.72.182.77}"

log() { echo "[cesarops2-fr] $*"; }

if [[ "$(id -u)" -ne 0 ]]; then
  log "Run with sudo for /etc/cesarops install, or re-run as root sections manually"
fi

log "Tailscale hostname=${TS_HOSTNAME}"
if command -v tailscale >/dev/null 2>&1; then
  if [[ -n "${TS_AUTHKEY:-}" ]]; then
    tailscale up --hostname="$TS_HOSTNAME" --ssh --authkey="$TS_AUTHKEY" || true
  else
    log "Join mesh: sudo tailscale up --hostname=${TS_HOSTNAME} --ssh"
    log "Or: TS_AUTHKEY=tskey-auth-... sudo -E bash $0"
  fi
  FR_IP=$(tailscale ip -4 2>/dev/null || true)
  log "This host Tailscale IPv4: ${FR_IP:-unknown}"
else
  log "Install Tailscale: curl -fsSL https://tailscale.com/install.sh | sh"
fi

sudo mkdir -p /etc/cesarops
sudo cp "${CFG}/cesarops-node.toml" /etc/cesarops/cesarops-node.toml
sudo cp "${CFG}/cesarops2-fr.env" /etc/cesarops/cesarops2-fr.env
if [[ -n "${FR_IP:-}" ]]; then
  echo "CESAROPS2_FR_TS_IP=${FR_IP}" | sudo tee -a /etc/cesarops/cesarops2-fr.env >/dev/null
  sudo sed -i "s/^CESAROPS2_FR_TS_IP=.*/CESAROPS2_FR_TS_IP=${FR_IP}/" /etc/cesarops/cesarops2-fr.env 2>/dev/null || true
fi

# NFS / repo paths (same as legacy cesarops2)
if ! mountpoint -q /mnt/t440 2>/dev/null; then
  log "Mount T440 share (adjust if your path differs):"
  log "  sudo mkdir -p /mnt/t440"
  log "  sudo mount -t nfs ${T440_TS_IP}:/codebase /mnt/t440"
fi

log "Installed /etc/cesarops/cesarops-node.toml + cesarops2-fr.env"
log "Start lab (after mount): bash ${REPO}/scripts/cesarops2_research_lab.sh start"
log "Cake workers: bash ${REPO}/scripts/cake/prep-c2-hetero-workers.sh"
log "On T440, patch Forge IP: bash ${REPO}/scripts/patch_cluster_config_cesarops2_fr.sh ${FR_IP:-<tailscale-ip>}"
