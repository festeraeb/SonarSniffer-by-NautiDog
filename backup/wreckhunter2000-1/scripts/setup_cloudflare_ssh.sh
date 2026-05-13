#!/bin/bash
# ═══════════════════════════════════════════════════════════════════════════════
# Cloudflare Tunnel SSH Access — Bypass School/Corporate Firewalls
# ═══════════════════════════════════════════════════════════════════════════════
# This adds SSH access through the existing Cloudflare tunnel.
# All traffic goes over HTTPS (port 443) — invisible to firewalls.
#
# Run ON the T440 (after reboot):
#   bash scripts/setup_cloudflare_ssh.sh
#
# Then on your LAPTOP (school network):
#   1. Install cloudflared: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/
#   2. Add the SSH config block (see bottom of this script)
#   3. ssh t440  (works through any firewall that allows HTTPS)
# ═══════════════════════════════════════════════════════════════════════════════

set -euo pipefail

TUNNEL_NAME="cesarops-main"
SSH_HOSTNAME="ssh.cesarops.org"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Cloudflare Tunnel — SSH Access Setup                        ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# ── Step 1: Check cloudflared is installed and tunnel exists ──────────────────
if ! command -v cloudflared &>/dev/null; then
    echo "[ERROR] cloudflared not installed. Install with:"
    echo "  curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 -o /usr/local/bin/cloudflared"
    echo "  chmod +x /usr/local/bin/cloudflared"
    exit 1
fi

echo "[1/4] cloudflared found: $(cloudflared --version 2>&1 | head -1)"

# ── Step 2: Add SSH ingress rule to tunnel config ─────────────────────────────
# cloudflared config is typically at /etc/cloudflared/config.yml or ~/.cloudflared/config.yml
CONFIG_PATHS=(
    "/etc/cloudflared/config.yml"
    "/home/cesarops/.cloudflared/config.yml"
    "/root/.cloudflared/config.yml"
)

CONFIG_FILE=""
for p in "${CONFIG_PATHS[@]}"; do
    if [ -f "$p" ]; then
        CONFIG_FILE="$p"
        break
    fi
done

if [ -z "$CONFIG_FILE" ]; then
    echo "[ERROR] Cannot find cloudflared config. Checking tunnel list..."
    cloudflared tunnel list 2>&1 | head -10
    echo ""
    echo "You may need to create the config manually."
    echo "Typical location: /etc/cloudflared/config.yml"
    exit 1
fi

echo "[2/4] Found config: $CONFIG_FILE"
echo ""

# Check if SSH rule already exists
if grep -q "$SSH_HOSTNAME" "$CONFIG_FILE" 2>/dev/null; then
    echo "[OK] SSH hostname ($SSH_HOSTNAME) already in tunnel config."
else
    echo "[2/4] Adding SSH ingress rule to $CONFIG_FILE..."
    echo ""
    echo "  Add this BEFORE the catch-all 404 rule in the ingress section:"
    echo ""
    echo "  - hostname: $SSH_HOSTNAME"
    echo "    service: ssh://localhost:22"
    echo ""
    echo "  Full example ingress block:"
    echo "  ─────────────────────────────────────────────"
    echo "  ingress:"
    echo "    - hostname: llm.cesarops.org"
    echo "      service: http://localhost:5001"
    echo "    - hostname: api.cesarops.org"
    echo "      service: http://localhost:8099"
    echo "    - hostname: ssh.cesarops.org"
    echo "      service: ssh://localhost:22"
    echo "    - service: http_status:404"
    echo "  ─────────────────────────────────────────────"
    echo ""
    echo "  After editing, restart the tunnel:"
    echo "    sudo systemctl restart cloudflared"
fi

# ── Step 3: DNS record ────────────────────────────────────────────────────────
echo ""
echo "[3/4] DNS setup:"
echo "  Create a CNAME record for $SSH_HOSTNAME pointing to your tunnel:"
echo ""
echo "    cloudflared tunnel route dns $TUNNEL_NAME $SSH_HOSTNAME"
echo ""
echo "  Or manually add in Cloudflare dashboard:"
echo "    Type: CNAME"
echo "    Name: ssh"
echo "    Target: <tunnel-id>.cfargotunnel.com"
echo ""

# ── Step 4: Client setup instructions ────────────────────────────────────────
echo "[4/4] CLIENT SETUP (your laptop at school):"
echo ""
echo "  1. Install cloudflared on your laptop:"
echo "     Windows: winget install cloudflare.cloudflared"
echo "     Mac:     brew install cloudflared"
echo "     Linux:   curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 -o /usr/local/bin/cloudflared && chmod +x /usr/local/bin/cloudflared"
echo ""
echo "  2. Add to ~/.ssh/config (or C:\\Users\\<you>\\.ssh\\config on Windows):"
echo ""
echo "    Host t440"
echo "        HostName $SSH_HOSTNAME"
echo "        User cesarops"
echo "        ProxyCommand cloudflared access ssh --hostname %h"
echo ""
echo "  3. Then just:"
echo "     ssh t440"
echo ""
echo "  That's it. All traffic goes through HTTPS/443 via Cloudflare's edge."
echo "  No Tailscale needed. No ports to open. Works behind any firewall."
echo ""
echo "═══════════════════════════════════════════════════════════════════"
echo ""
echo "  BONUS: You can also access the LLM API directly at:"
echo "    https://llm.cesarops.org/v1/chat/completions"
echo ""
echo "  And if you want VS Code Remote SSH through the tunnel:"
echo "    Same ~/.ssh/config entry works — VS Code uses it automatically."
echo ""
echo "═══════════════════════════════════════════════════════════════════"
