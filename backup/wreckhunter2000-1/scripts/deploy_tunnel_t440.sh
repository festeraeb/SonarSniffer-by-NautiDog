#!/bin/bash
# ═══════════════════════════════════════════════════════════════════════════════
# Deploy Cloudflare Tunnel Config to T440
# ═══════════════════════════════════════════════════════════════════════════════
# Moves all tunnel routing from the old i7 to T440.
# Adds SSH access, frontend serving, and fixes the app.cesarops.org route.
#
# Run FROM your dev machine (not on T440):
#   bash scripts/deploy_tunnel_t440.sh
#
# Prerequisites:
#   - SSH access to T440 (Tailscale or LAN)
#   - cloudflared already installed on T440 (it is — service is running)
#   - Tunnel credentials already on T440 (they are — tunnel was working)
# ═══════════════════════════════════════════════════════════════════════════════

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
HOST="${T440_TAILSCALE:-100.72.182.77}"
USER="${T440_USER:-cesarops}"
SUDO_PASS="${SUDO_PASS:-cesarops}"

SSH_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10"
ssh_run() { ssh $SSH_OPTS "$USER@$HOST" "$@"; }

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Deploy Cloudflare Tunnel → T440 (all services)             ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# ── Check connectivity ────────────────────────────────────────────────────────
echo "[1/6] Checking SSH to $HOST..."
if ! ssh_run "true" 2>/dev/null; then
    echo "[ERROR] Cannot reach T440 at $HOST"
    exit 1
fi
echo "  ✓ Connected"

# ── Find existing tunnel config and credentials ──────────────────────────────
echo ""
echo "[2/6] Locating existing tunnel config..."
TUNNEL_INFO=$(ssh_run "
    # Find the credentials file (has the tunnel ID)
    CRED=\$(find /etc/cloudflared /home/cesarops/.cloudflared /root/.cloudflared -name '*.json' -not -name 'config.json' 2>/dev/null | head -1)
    CONFIG=\$(find /etc/cloudflared /home/cesarops/.cloudflared /root/.cloudflared -name 'config.yml' 2>/dev/null | head -1)
    echo \"CRED=\$CRED\"
    echo \"CONFIG=\$CONFIG\"
    # Get tunnel ID from credentials filename
    if [ -n \"\$CRED\" ]; then
        TUNNEL_ID=\$(basename \"\$CRED\" .json)
        echo \"TUNNEL_ID=\$TUNNEL_ID\"
    fi
")
echo "$TUNNEL_INFO"

# Extract values
CRED_FILE=$(echo "$TUNNEL_INFO" | grep "^CRED=" | cut -d= -f2)
CONFIG_FILE=$(echo "$TUNNEL_INFO" | grep "^CONFIG=" | cut -d= -f2)
TUNNEL_ID=$(echo "$TUNNEL_INFO" | grep "^TUNNEL_ID=" | cut -d= -f2)

if [ -z "$CRED_FILE" ]; then
    echo "[ERROR] No tunnel credentials found. Is cloudflared configured?"
    exit 1
fi

echo "  Credentials: $CRED_FILE"
echo "  Config: $CONFIG_FILE"
echo "  Tunnel ID: $TUNNEL_ID"

# ── Deploy new config ─────────────────────────────────────────────────────────
echo ""
echo "[3/6] Deploying updated tunnel config..."

# Upload the config, substituting the actual tunnel ID and credentials path
ssh_run "echo '$SUDO_PASS' | sudo -S tee /etc/cloudflared/config.yml > /dev/null" << EOF
tunnel: $TUNNEL_ID
credentials-file: $CRED_FILE

originRequest:
  noTLSVerify: true

ingress:
  # LLM API (KoboldCPP on port 5001, or Cake on 5002)
  - hostname: llm.cesarops.org
    service: http://localhost:5001
    originRequest:
      connectTimeout: 120s

  # Wrecks REST API
  - hostname: api.cesarops.org
    service: http://localhost:8099

  # Frontend Web App (nginx on T440, cesarops2 as backup)
  - hostname: app.cesarops.org
    service: http://localhost:8080

  # SSH access (bypasses school firewalls)
  - hostname: ssh.cesarops.org
    service: ssh://localhost:22

  # Catch-all
  - service: http_status:404
EOF
echo "  ✓ Config deployed"

# ── Set up nginx for frontend ─────────────────────────────────────────────────
echo ""
echo "[4/6] Setting up nginx for app.cesarops.org..."
ssh_run "
    echo '$SUDO_PASS' | sudo -S bash -c '
    # Install nginx if not present
    if ! command -v nginx &>/dev/null; then
        apt-get update -qq && apt-get install -y nginx
    fi

    # Create web root
    mkdir -p /var/www/cesarops

    # Write nginx config
    cat > /etc/nginx/sites-available/cesarops <<NGINX
server {
    listen 8080 default_server;
    server_name app.cesarops.org;
    root /var/www/cesarops;
    index index.html;

    # SPA fallback
    location / {
        try_files \\\$uri \\\$uri/ /index.html;
    }

    # Cache static assets
    location ~* \\.(js|css|png|jpg|jpeg|gif|ico|svg|woff2?)$ {
        expires 7d;
        add_header Cache-Control \"public, immutable\";
    }
}
NGINX

    # Enable site
    ln -sf /etc/nginx/sites-available/cesarops /etc/nginx/sites-enabled/cesarops
    rm -f /etc/nginx/sites-enabled/default 2>/dev/null || true

    # Test and reload
    nginx -t && systemctl reload nginx || systemctl start nginx
    systemctl enable nginx
    '
"
echo "  ✓ nginx configured on port 8080"

# ── Add DNS route for ssh.cesarops.org ────────────────────────────────────────
echo ""
echo "[5/6] Adding SSH DNS route..."
ssh_run "cloudflared tunnel route dns $TUNNEL_ID ssh.cesarops.org 2>&1 || echo '(may already exist)'"
echo "  ✓ DNS route set (or already exists)"

# ── Restart cloudflared ───────────────────────────────────────────────────────
echo ""
echo "[6/6] Restarting cloudflared..."
ssh_run "echo '$SUDO_PASS' | sudo -S systemctl restart cloudflared"
sleep 3
ssh_run "systemctl is-active cloudflared"
echo "  ✓ Tunnel restarted"

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════════════════════════"
echo ""
echo "  All services now route through T440:"
echo ""
echo "    https://llm.cesarops.org/v1    → KoboldCPP (port 5001)"
echo "    https://api.cesarops.org       → wrecks-api (port 8099)"
echo "    https://app.cesarops.org       → nginx/frontend (port 8080)"
echo "    ssh.cesarops.org               → SSH (port 22)"
echo ""
echo "  SSH from school (add to ~/.ssh/config):"
echo ""
echo "    Host t440"
echo "        HostName ssh.cesarops.org"
echo "        User cesarops"
echo "        ProxyCommand cloudflared access ssh --hostname %h"
echo ""
echo "  Deploy frontend to T440:"
echo "    scp -r tauri/dist-web/* cesarops@100.72.182.77:/var/www/cesarops/"
echo ""
echo "  cesarops2 backup LLM (when T440 is busy/swapping):"
echo "    KoboldCPP on cesarops2:5001 with smaller models"
echo ""
echo "═══════════════════════════════════════════════════════════════════"
