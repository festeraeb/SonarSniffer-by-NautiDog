#!/usr/bin/env bash
# setup_cloudflare_tunnel.sh  —  run this on the i7 node (10.0.0.56)
# Exposes the wrecks API on a public *.trycloudflare.com URL so you can
# reach it from anywhere (school, phone, etc.) — no port-forwarding needed.
#
# ─── Option A: Quick tunnel (no Cloudflare account, URL changes every restart) ──
# Run:   bash setup_cloudflare_tunnel.sh
# Copy the printed URL, then set API_BASE_URL in .env to that URL.
#
# ─── Option B: Named tunnel (persistent URL, free Cloudflare account required) ─
# Run:   bash setup_cloudflare_tunnel.sh --named
# Follow the prompts to log in and pick a hostname.
# ────────────────────────────────────────────────────────────────────────────────

set -euo pipefail

WRECKS_PORT="${I7_TPU_PORT:-5001}"
NAMED="${1:-}"

install_cloudflared() {
    if command -v cloudflared &>/dev/null; then
        echo "[✓] cloudflared already installed: $(cloudflared --version)"
        return
    fi
    echo "[→] Installing cloudflared …"
    ARCH=$(dpkg --print-architecture 2>/dev/null || echo amd64)
    TMP=$(mktemp -d)
    curl -fsSL "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-${ARCH}.deb" \
         -o "${TMP}/cloudflared.deb"
    sudo dpkg -i "${TMP}/cloudflared.deb"
    rm -rf "${TMP}"
    echo "[✓] cloudflared installed: $(cloudflared --version)"
}

install_cloudflared

if [[ "${NAMED}" == "--named" ]]; then
    # ── Persistent named tunnel ─────────────────────────────────────────────
    echo ""
    echo "=== Named tunnel setup ==="
    echo "A browser window will open (or copy the link) to log in to Cloudflare."
    cloudflared tunnel login
    echo ""
    read -rp "Enter the tunnel name (e.g. wreckhunter): " TUNNEL_NAME
    cloudflared tunnel create "${TUNNEL_NAME}"
    TUNNEL_ID=$(cloudflared tunnel list | awk -v name="${TUNNEL_NAME}" '$2==name {print $1}')
    echo "[✓] Tunnel ID: ${TUNNEL_ID}"

    mkdir -p ~/.cloudflared
    cat > ~/.cloudflared/config.yml <<EOF
tunnel: ${TUNNEL_ID}
credentials-file: /root/.cloudflared/${TUNNEL_ID}.json

ingress:
  - service: http://localhost:${WRECKS_PORT}
EOF

    read -rp "Enter the hostname to use (e.g. wrecks.yourdomain.com): " HOSTNAME
    cloudflared tunnel route dns "${TUNNEL_NAME}" "${HOSTNAME}"
    echo ""
    echo "=== Installing as systemd service ==="
    sudo cloudflared service install
    sudo systemctl enable --now cloudflared
    echo ""
    echo "[✓] Named tunnel running."
    echo "    Public URL : https://${HOSTNAME}"
    echo "    Add to .env: API_BASE_URL=https://${HOSTNAME}"

else
    # ── Quick tunnel (no login, random *.trycloudflare.com URL) ─────────────
    echo ""
    echo "=== Quick tunnel (temporary URL) ==="
    echo "Starting quick tunnel on port ${WRECKS_PORT} …"
    echo "Press Ctrl+C to stop."
    echo ""
    echo "Once the URL appears, set it in your .env on the dev machine:"
    echo "  API_BASE_URL=https://<random>.trycloudflare.com"
    echo ""
    echo "Then in Google Earth, open:"
    echo "  https://<random>.trycloudflare.com/wrecks/networklink.kmz"
    echo ""
    cloudflared tunnel --url "http://localhost:${WRECKS_PORT}"
fi
