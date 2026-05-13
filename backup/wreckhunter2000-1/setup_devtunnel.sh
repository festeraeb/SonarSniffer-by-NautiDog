#!/usr/bin/env bash
# setup_devtunnel.sh  —  run this on the i7 node (10.0.0.56)
#
# Azure Dev Tunnels expose a local port through Microsoft's infrastructure
# via outbound HTTPS only — no inbound firewall rules, no port forwarding.
# Endpoint looks like: https://abc123-5001.devtunnels.ms
#
# Requires an Azure / Microsoft account (free with Azure for Students).
# Unlike Cloudflare quick tunnels, the URL is stable across reboots once
# the tunnel is named, and the service auto-starts ─ works even from school.
#
# Usage:
#   First run:   bash setup_devtunnel.sh --login
#   After login: bash setup_devtunnel.sh
# ────────────────────────────────────────────────────────────────────────────

set -euo pipefail

WRECKS_PORT="${I7_TPU_PORT:-5001}"
TUNNEL_NAME="wreckhunter"
LOGIN="${1:-}"

install_devtunnel() {
    if command -v devtunnel &>/dev/null; then
        echo "[✓] devtunnel already installed: $(devtunnel --version 2>/dev/null || true)"
        return
    fi
    echo "[→] Installing devtunnel CLI …"
    curl -sL https://aka.ms/DevTunnelCliInstall | bash
    export PATH="$HOME/.local/bin:$PATH"
    echo "[✓] devtunnel installed."
}

install_devtunnel

if [[ "${LOGIN}" == "--login" ]]; then
    echo ""
    echo "=== Logging in to Azure Dev Tunnels ==="
    echo "This opens a browser (or gives a device-code URL)."
    devtunnel user login
    echo "[✓] Logged in."
fi

# Create a named persistent tunnel (idempotent)
echo ""
echo "=== Creating persistent tunnel '${TUNNEL_NAME}' on port ${WRECKS_PORT} ==="
devtunnel delete "${TUNNEL_NAME}" 2>/dev/null || true
devtunnel create "${TUNNEL_NAME}" --allow-anonymous
devtunnel port create "${TUNNEL_NAME}" --port-number "${WRECKS_PORT}" --protocol https

TUNNEL_URL=$(devtunnel show "${TUNNEL_NAME}" 2>/dev/null | grep -oP 'https://[^\s]+devtunnels\.ms[^\s]*' | head -1 || true)
echo ""
echo "[✓] Tunnel URL: ${TUNNEL_URL:-<run `devtunnel show ${TUNNEL_NAME}` to get URL>}"
echo "    Set in .env: API_BASE_URL=${TUNNEL_URL:-https://YOUR_TUNNEL.devtunnels.ms}"
echo ""
echo "In Google Earth: File → Open → paste:"
echo "    ${TUNNEL_URL:-https://YOUR_TUNNEL.devtunnels.ms}/${WRECKS_PORT}/wrecks/networklink.kmz"

# ── Install as systemd service so it auto-starts on boot ───────────────────
echo ""
echo "=== Installing as systemd service (auto-starts on boot) ==="

DEVTUNNEL_BIN="$(command -v devtunnel)"
SERVICE_FILE="/etc/systemd/system/devtunnel-wreckhunter.service"

sudo tee "${SERVICE_FILE}" > /dev/null <<EOF
[Unit]
Description=Azure Dev Tunnel – wreckhunter API
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=cesarops
Environment=HOME=/home/cesarops
ExecStart=${DEVTUNNEL_BIN} host ${TUNNEL_NAME}
Restart=on-failure
RestartSec=15

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable devtunnel-wreckhunter
sudo systemctl restart devtunnel-wreckhunter
echo "[✓] Service enabled. Tunnel will start automatically on boot."
echo ""
echo "Commands:"
echo "  sudo systemctl status devtunnel-wreckhunter"
echo "  sudo journalctl -u devtunnel-wreckhunter -f"
