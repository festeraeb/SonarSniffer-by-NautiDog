#!/usr/bin/env bash
# setup_tailscale_exit_node.sh
#
# Provisions a cheap Azure VM, installs Tailscale + nginx, and configures it
# as both a Tailscale exit node and a reverse proxy for all wreckhunter services.
#
# How it works:
#   School laptop ──SSH/HTTP──▶ Azure VM (public IP) ──Tailscale mesh──▶ i7 (100.x.x.x)
#   VSCode Remote SSH:  school ──▶ Azure VM ──ProxyJump──▶ i7 ──ProxyJump──▶ Pi (10.0.0.226)
#
# Exposed services via nginx:
#   /          → i7:5173  (Tauri / Vite dev server)
#   /wrecks/*  → i7:5001  (wrecks FastAPI — KML, REST)
#   /api/*     → i7:5001  (wrecks FastAPI — REST alias)
#
# VSCode Remote SSH (after running setup_vscode_ssh.ps1):
#   Host i7   → cesarops@<i7-tailscale-ip>  via ProxyJump Azure VM
#   Host pi   → pi@10.0.0.226               via ProxyJump i7 (double-hop, no TS on Pi needed)
#
# Prerequisites:
#   - az CLI installed and logged in (az login)
#   - Tailscale account — auth key from https://login.tailscale.com/admin/settings/keys
#   - i7 must be on Tailscale: sudo tailscale up --ssh
#   - Know your i7's Tailscale IP: tailscale ip -4  (on i7)
#
# Usage:
#   TS_AUTHKEY="tskey-auth-xxxxx" I7_TS_IP="100.x.x.x" bash setup_tailscale_exit_node.sh
# ────────────────────────────────────────────────────────────────────────────

set -euo pipefail

# ── Config (override with env vars) ─────────────────────────────────────────
RG="${AZURE_RG:-wreckhunter-rg}"
LOCATION="${AZURE_LOCATION:-eastus}"
VM_NAME="${AZURE_VM_NAME:-wreckhunter-exit}"
VM_SIZE="${AZURE_VM_SIZE:-Standard_B1s}"   # ~$0.012/hr, well within student credit
ADMIN_USER="cesarops"
SSH_KEY_FILE="${HOME}/.ssh/id_ed25519.pub"

# REQUIRED — set these before running
TS_AUTHKEY="${TS_AUTHKEY:?Set TS_AUTHKEY to a Tailscale reusable/ephemeral auth key}"
I7_TS_IP="${I7_TS_IP:?Set I7_TS_IP to i7's Tailscale IP (run: tailscale ip -4 on i7)}"
WRECKS_PORT="${WRECKS_PORT:-5001}"
TAURI_PORT="${TAURI_PORT:-5173}"       # Vite dev server (tauri dev)
PI_LOCAL_IP="${PI_LOCAL_IP:-10.0.0.226}"  # Pi is reachable from i7 via LAN

echo ""
echo "=== Step 1: Create resource group + VM ==="
az group create --name "${RG}" --location "${LOCATION}" --output none

PUBLIC_IP=$(az vm create \
  --resource-group "${RG}" \
  --name "${VM_NAME}" \
  --image Ubuntu2404 \
  --size "${VM_SIZE}" \
  --admin-username "${ADMIN_USER}" \
  --ssh-key-values "${SSH_KEY_FILE}" \
  --public-ip-sku Standard \
  --public-ip-address-allocation static \
  --nsg-rule SSH \
  --output tsv \
  --query publicIpAddress)

echo "[✓] VM created: ${PUBLIC_IP}"

# Open port 443 for HTTPS (nginx TLS) and 80 for ACME challenge
az vm open-port --resource-group "${RG}" --name "${VM_NAME}" --port 443 --priority 900 --output none
az vm open-port --resource-group "${RG}" --name "${VM_NAME}" --port 80  --priority 910 --output none

echo ""
echo "=== Step 2: Bootstrap VM (Tailscale + nginx + reverse proxy) ==="
ssh -o StrictHostKeyChecking=no -o ConnectTimeout=30 \
    "${ADMIN_USER}@${PUBLIC_IP}" \
    TS_AUTHKEY="${TS_AUTHKEY}" \
    I7_TS_IP="${I7_TS_IP}" \
    WRECKS_PORT="${WRECKS_PORT}" \
    TAURI_PORT="${TAURI_PORT}" \
    PUBLIC_IP="${PUBLIC_IP}" \
    'bash -s' << 'REMOTE'

set -euo pipefail

# ── Tailscale ────────────────────────────────────────────────────────────────
echo "[→] Installing Tailscale …"
curl -fsSL https://tailscale.com/install.sh | sh
sudo systemctl enable --now tailscaled

echo "[→] Connecting to Tailscale as exit node …"
sudo tailscale up \
  --authkey="${TS_AUTHKEY}" \
  --advertise-exit-node \
  --ssh \
  --hostname="wreckhunter-exit"

# Allow IP forwarding (required for exit node)
echo 'net.ipv4.ip_forward=1'             | sudo tee -a /etc/sysctl.d/99-tailscale.conf
echo 'net.ipv6.conf.all.forwarding=1'    | sudo tee -a /etc/sysctl.d/99-tailscale.conf
sudo sysctl -p /etc/sysctl.d/99-tailscale.conf

# ── nginx — full reverse proxy ────────────────────────────────────────────────
echo "[→] Installing nginx …"
sudo apt-get update -qq
sudo apt-get install -y nginx 2>&1 | tail -3

sudo tee /etc/nginx/sites-available/wreckhunter << EOF
# Named upstreams — all traffic goes through Tailscale mesh to i7
upstream wrecks_api {
    server ${I7_TS_IP}:${WRECKS_PORT};
    keepalive 16;
}

upstream tauri_dev {
    server ${I7_TS_IP}:${TAURI_PORT};
    keepalive 8;
}

server {
    listen 80;
    server_name ${PUBLIC_IP} _;

    # ── Wrecks REST API + KML endpoints ──────────────────────────────
    location /wrecks/ {
        proxy_pass         http://wrecks_api;
        proxy_set_header   Host \$host;
        proxy_set_header   X-Real-IP \$remote_addr;
        proxy_set_header   X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_read_timeout 120;
        proxy_buffering    off;
        proxy_hide_header  Content-Disposition;
        add_header         Access-Control-Allow-Origin * always;
    }

    location /api/ {
        proxy_pass         http://wrecks_api;
        proxy_set_header   Host \$host;
        proxy_set_header   X-Real-IP \$remote_addr;
        proxy_set_header   X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_read_timeout 120;
        proxy_buffering    off;
        add_header         Access-Control-Allow-Origin * always;
    }

    # ── Tauri / Vite dev server (port ${TAURI_PORT}) ──────────────────
    # Includes WebSocket passthrough for Vite HMR
    location / {
        proxy_pass         http://tauri_dev;
        proxy_set_header   Host \$host;
        proxy_set_header   X-Real-IP \$remote_addr;
        proxy_set_header   X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_read_timeout 120;
        proxy_buffering    off;

        # WebSocket (Vite HMR)
        proxy_http_version 1.1;
        proxy_set_header   Upgrade \$http_upgrade;
        proxy_set_header   Connection "upgrade";
    }
}
EOF

sudo ln -sf /etc/nginx/sites-available/wreckhunter /etc/nginx/sites-enabled/wreckhunter
sudo rm -f /etc/nginx/sites-enabled/default
sudo nginx -t && sudo systemctl reload nginx

echo ""
echo "================================================================"
echo "  Proxy ready on http://${PUBLIC_IP}"
echo "  /          → i7:${TAURI_PORT}  (Tauri dev UI)"
echo "  /wrecks/*  → i7:${WRECKS_PORT}  (wrecks API / KML)"
echo "  /api/*     → i7:${WRECKS_PORT}  (REST alias)"
echo "================================================================"
REMOTE

echo ""
echo "=== Done ==="
echo ""
echo "  VM public IP : ${PUBLIC_IP}"
echo "  Monthly cost : ~\$8-9 (Standard_B1s) — well within student credit"
echo ""
echo "Action items:"
echo ""
echo "  1. On i7 (when home):"
echo "       sudo tailscale up --ssh"
echo "       cd ~/wreckhunter2000-1 && npm --prefix tauri run tauri dev &"
echo ""
echo "  2. Approve exit node in Tailscale admin:"
echo "       https://login.tailscale.com/admin/machines → wreckhunter-exit → 'Use as exit node'"
echo ""
echo "  3. Set in .env:"
echo "       API_BASE_URL=http://${PUBLIC_IP}"
echo ""
echo "  4. Run SSH config script on Windows (VSCode Remote SSH):"
echo "       scripts\\setup_vscode_ssh.ps1 -AzureVMIP ${PUBLIC_IP} -I7TailscaleIP ${I7_TS_IP} -PiLocalIP ${PI_LOCAL_IP}"
echo ""
echo "     Then in VSCode: Remote Explorer → SSH → i7  (or pi)"
echo ""
echo "  5. Browser URLs from school:"
echo "       Tauri UI   : http://${PUBLIC_IP}/"
echo "       KML feed   : http://${PUBLIC_IP}/wrecks/live.kml"
echo "       GE download: http://${PUBLIC_IP}/wrecks/networklink.kmz"
echo ""
echo "To tear down when no longer needed:"
echo "  az group delete --name ${RG} --yes --no-wait"
