#!/usr/bin/env bash
# Run on cesarops2-fr (France) when it has internet — pulls configs from T440.
# Example (from FR host):
#   curl -fsSL "http://100.72.182.77:8877/config/cesarops2-fr/bootstrap-from-t440.sh" | bash
# Or after Tailscale to T440 LAN:
#   curl -fsSL "http://10.0.0.61:8877/config/cesarops2-fr/bootstrap-from-t440.sh" | bash
set -euo pipefail

T440_URL="${T440_URL:-http://100.72.182.77:8877}"
TS_HOSTNAME="${TS_HOSTNAME:-cesarops2-fr}"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"

log() { echo "[bootstrap-c2-fr] $*"; }

log "Fetching config bundle from T440 (${T440_URL})…"
mkdir -p "${REPO}/config/cesarops2-fr"
for f in cesarops-node.toml cesarops2-fr.env cluster_config.snippet.toml; do
  curl -fsSL "${T440_URL}/config/cesarops2-fr/${f}" -o "${REPO}/config/cesarops2-fr/${f}"
done

if [[ -f "${REPO}/scripts/setup_cesarops2_fr_tailscale.sh" ]]; then
  SETUP="${REPO}/scripts/setup_cesarops2_fr_tailscale.sh"
else
  curl -fsSL "${T440_URL}/scripts/setup_cesarops2_fr_tailscale.sh" -o /tmp/setup_cesarops2_fr_tailscale.sh
  SETUP=/tmp/setup_cesarops2_fr_tailscale.sh
  chmod +x "$SETUP"
fi

log "Installing Tailscale + /etc/cesarops (need TS_AUTHKEY or interactive login)…"
export TS_HOSTNAME REPO
bash "$SETUP"

FR_IP=$(tailscale ip -4 2>/dev/null || true)
log "Done. FR Tailscale IP: ${FR_IP:-run: tailscale ip -4}"
log "Ask T440 to run: bash ${REPO}/scripts/patch_cluster_config_cesarops2_fr.sh ${FR_IP}"
