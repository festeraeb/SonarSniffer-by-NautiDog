#!/usr/bin/env bash
# setup_cesarops_tunnel.sh — Run on T440 (10.0.0.61 / 100.72.182.77)
#
# Installs cloudflared and connects the T440 to the cesarops-main tunnel.
# Ingress rules are managed via Cloudflare API (already configured):
#   api.cesarops.org  → localhost:8099  (wrecks-api / FastAPI)
#   llm.cesarops.org  → localhost:5001  (KoboldCPP inference)
#   cesarops.org      → localhost:8099  (root → API)
#   www.cesarops.org  → localhost:8099
#
# Usage:
#   bash scripts/setup_cesarops_tunnel.sh
#
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

# cesarops-main tunnel token (from Cloudflare API)
TUNNEL_TOKEN="eyJhIjoiNGJhZmRkZmFhMGUxZGE4MWI1YWVhZmVhMmZlODg3OGUiLCJ0IjoiMDYyYjhmZGMtYzI1Zi00NmI1LTkxZTgtNDI5NTM5NzcxYjgwIiwicyI6IlkyVnpZWEp2Y0hNdGJXRnBiaTEwZFc1dVpXd3RjMlZqY21WMExUSXdNalloIn0="

# ── Install cloudflared if missing ───────────────────────────────────────────
if command -v cloudflared &>/dev/null; then
    echo "[✓] cloudflared $(cloudflared --version 2>&1 | head -1)"
else
    echo "[→] Installing cloudflared …"
    ARCH=$(dpkg --print-architecture 2>/dev/null || echo amd64)
    TMP=$(mktemp -d)
    curl -fsSL "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-${ARCH}.deb" \
         -o "${TMP}/cloudflared.deb"
    sudo dpkg -i "${TMP}/cloudflared.deb"
    rm -rf "${TMP}"
    echo "[✓] cloudflared installed"
fi

# ── Install as systemd service with token ────────────────────────────────────
echo "[→] Configuring systemd service …"

sudo tee /etc/systemd/system/cloudflared.service > /dev/null << EOF
[Unit]
Description=Cloudflare Tunnel (cesarops-main)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/bin/cloudflared --no-autoupdate tunnel run --token ${TUNNEL_TOKEN}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable cloudflared
sudo systemctl restart cloudflared

echo ""
echo "══════════════════════════════════════════════════════════════════"
echo "  ✓ DONE — Tunnel is live!"
echo ""
echo "  Public endpoints (once connected):"
echo "    https://api.cesarops.org    → wrecks-api (:8099)"
echo "    https://llm.cesarops.org    → KoboldCPP  (:5001)"
echo "    https://cesarops.org        → wrecks-api (:8099)"
echo ""
echo "  Manage:"
echo "    sudo systemctl status cloudflared"
echo "    sudo systemctl restart cloudflared"
echo "══════════════════════════════════════════════════════════════════"
