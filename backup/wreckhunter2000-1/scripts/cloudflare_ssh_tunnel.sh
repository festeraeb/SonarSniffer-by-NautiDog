#!/usr/bin/env bash
# cloudflare_ssh_tunnel.sh — Run ON the i7 (10.0.0.56) to expose SSH over Cloudflare
#
# No Cloudflare account needed for the quick tunnel test.
# Cloudflare connects outbound on port 443 (HTTPS) — works through most school firewalls.
#
# USAGE:
#   bash cloudflare_ssh_tunnel.sh
#
# OUTPUT: prints a URL like  https://abc-def-ghi.trycloudflare.com
#   Copy that URL, then on your school laptop run:
#     ssh -o ProxyCommand="cloudflared access tcp --hostname abc-def-ghi.trycloudflare.com" cesarops@abc-def-ghi.trycloudflare.com
# ─────────────────────────────────────────────────────────────────────────────

set -e

# Install cloudflared if missing
if ! command -v cloudflared &>/dev/null; then
    echo "Installing cloudflared..."
    curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg \
        | sudo tee /usr/share/keyrings/cloudflare-main.gpg > /dev/null
    echo "deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] \
https://pkg.cloudflare.com/cloudflared $(lsb_release -cs) main" \
        | sudo tee /etc/apt/sources.list.d/cloudflared.list
    sudo apt-get update -qq
    sudo apt-get install -y cloudflared
fi

echo ""
echo "Starting Cloudflare quick tunnel for SSH (TCP port 22)..."
echo "This URL is temporary — it changes every time you run this."
echo ""
echo "════════════════════════════════════════════════════════════════"
echo " When you see the tunnel URL below, run this on your SCHOOL laptop:"
echo ""
echo '   cloudflared access tcp --hostname <URL> --url localhost:2222 &'
echo '   ssh -p 2222 -o StrictHostKeyChecking=no cesarops@localhost'
echo ""
echo " OR in one line (requires cloudflared on school laptop):"
echo '   ssh -o ProxyCommand="cloudflared access tcp --hostname <URL>" cesarops@<URL>'
echo "════════════════════════════════════════════════════════════════"
echo ""

# Start the quick tunnel — it prints the URL to stderr
cloudflared tunnel --url tcp://localhost:22 2>&1
