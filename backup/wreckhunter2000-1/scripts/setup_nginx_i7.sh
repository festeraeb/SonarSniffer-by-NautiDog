#!/usr/bin/env bash
# setup_nginx_i7.sh — Run ON the i7 (10.0.0.56) to expose wrecks API publicly
#
# Usage:
#   bash setup_nginx_i7.sh                          # nginx + API service only
#   bash setup_nginx_i7.sh <IONOS_API_KEY>           # + IONOS DDNS for api.cesarops.com
#
# IONOS_API_KEY — from IONOS Developer Portal:
#   1. Go to: https://developer.hosting.ionos.com
#   2. Create API key → copy the combined "prefix.secret" string
#
# After this script:
#   1. Port-forward TCP 80 → 10.0.0.56:80 on your home router
#   2. The DDNS cron auto-creates/updates:  A  api.cesarops.com → your home IP
#   3. Load http://api.cesarops.com/wrecks/networklink.kmz into Google Earth Web
#   4. Web frontend: VITE_API_BASE=http://api.cesarops.com npx wrangler pages deploy dist
# ─────────────────────────────────────────────────────────────────────────────

set -e

IONOS_API_KEY="${1:-}"  # IONOS DNS API key (prefix.secret)
IONOS_SUBDOMAIN="api"   # api.cesarops.com
IONOS_DOMAIN="cesarops.com"

WRECKS_API_PORT=5001          # FastAPI wrecks API local port
API_USER="cesarops"           # user that runs the API (for systemd service)
REPO_DIR="/home/cesarops/wreckhunter2000-1"

echo "=== 1. Install nginx ==="
sudo apt-get update -qq
sudo apt-get install -y nginx

echo "=== 2. Write nginx config ==="
sudo tee /etc/nginx/sites-available/wreckhunter > /dev/null << 'NGINX'
# Wreckhunter API — public reverse proxy
# Serves: /wrecks/* → FastAPI on localhost:5001

server {
    listen 80;
    server_name _;

    # Security headers
    add_header X-Content-Type-Options nosniff;
    add_header X-Frame-Options SAMEORIGIN;

    # KML / API endpoints
    location /wrecks/ {
        proxy_pass         http://127.0.0.1:5001;
        proxy_set_header   Host $host;
        proxy_set_header   X-Real-IP $remote_addr;
        proxy_read_timeout 30s;

        # CORS — Google Earth Web requires these
        add_header Access-Control-Allow-Origin  "*" always;
        add_header Access-Control-Allow-Methods "GET, OPTIONS" always;
        add_header Access-Control-Allow-Headers "Origin, Accept" always;

        if ($request_method = OPTIONS) {
            return 204;
        }
    }

    # Health check
    location /health {
        proxy_pass http://127.0.0.1:5001/health;
    }

    # Block everything else
    location / {
        return 403;
    }
}
NGINX

sudo ln -sf /etc/nginx/sites-available/wreckhunter /etc/nginx/sites-enabled/wreckhunter
sudo rm -f /etc/nginx/sites-enabled/default
sudo nginx -t
sudo systemctl enable nginx
sudo systemctl restart nginx
echo "nginx OK"

echo "=== 3. Create wrecks-api systemd service ==="
sudo tee /etc/systemd/system/wrecks-api.service > /dev/null << UNIT
[Unit]
Description=Wreckhunter FastAPI
After=network.target

[Service]
User=${API_USER}
WorkingDirectory=${REPO_DIR}
ExecStart=/home/${API_USER}/tpu-venv/bin/uvicorn wrecks_api.app:app --host 127.0.0.1 --port ${WRECKS_API_PORT} --workers 2
Restart=always
RestartSec=5
Environment=PYTHONUNBUFFERED=1

[Install]
WantedBy=multi-user.target
UNIT

sudo systemctl daemon-reload
sudo systemctl enable wrecks-api
sudo systemctl restart wrecks-api
sleep 2
sudo systemctl status wrecks-api --no-pager | head -8

echo "=== 4. Firewall — allow HTTP ==="
sudo ufw allow 80/tcp 2>/dev/null || true
sudo ufw allow 22/tcp 2>/dev/null || true

echo "=== 5. IONOS DDNS for api.cesarops.com ==="
if [ -n "$IONOS_API_KEY" ]; then
    DDNS_SCRIPT="/home/${API_USER}/ionos-ddns.sh"
    cat > "$DDNS_SCRIPT" << 'IONOSDDNS'
#!/bin/bash
# IONOS DDNS updater — keeps api.cesarops.com pointing at home IP
# IONOS DNS API: https://developer.hosting.ionos.com/docs/dns
IONOS_KEY="PLACEHOLDER_KEY"
SUBDOMAIN="PLACEHOLDER_SUB"
DOMAIN="PLACEHOLDER_DOMAIN"
FQDN="${SUBDOMAIN}.${DOMAIN}"
API="https://api.hosting.ionos.com/dns/v1"

IP=$(curl -sf https://api.ipify.org 2>/dev/null) || exit 0

# Get zone ID for the domain
ZONE_ID=$(curl -sf -X GET "${API}/zones" \
  -H "X-API-Key: ${IONOS_KEY}" -H "Accept: application/json" \
  | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)

[ -z "$ZONE_ID" ] && echo "$(date -u) ERROR: zone not found" && exit 1

# Upsert the A record (IONOS PATCH replaces all records of same name+type)
curl -sf -X PATCH "${API}/zones/${ZONE_ID}" \
  -H "X-API-Key: ${IONOS_KEY}" -H "Content-Type: application/json" \
  --data "[{\"name\":\"${FQDN}\",\"type\":\"A\",\"content\":\"${IP}\",\"ttl\":300,\"prio\":0,\"disabled\":false}]" \
  > /dev/null

echo "$(date -u +%FT%TZ) updated ${FQDN} → ${IP}"
IONOSDDNS

    # Inject actual values (avoid heredoc variable expansion issues)
    sed -i "s|PLACEHOLDER_KEY|${IONOS_API_KEY}|g" "$DDNS_SCRIPT"
    sed -i "s|PLACEHOLDER_SUB|${IONOS_SUBDOMAIN}|g" "$DDNS_SCRIPT"
    sed -i "s|PLACEHOLDER_DOMAIN|${IONOS_DOMAIN}|g" "$DDNS_SCRIPT"

    chmod 700 "$DDNS_SCRIPT"
    chown ${API_USER}:${API_USER} "$DDNS_SCRIPT"

    # Run once now to create/update the DNS record
    bash "$DDNS_SCRIPT"

    # Cron every 5 minutes
    (crontab -u ${API_USER} -l 2>/dev/null; echo "*/5 * * * * ${DDNS_SCRIPT} >> /home/${API_USER}/ionos-ddns.log 2>&1") \
        | sort -u | crontab -u ${API_USER} -
    echo "IONOS DDNS active: ${IONOS_SUBDOMAIN}.${IONOS_DOMAIN}"
else
    echo "(skipped — no IONOS_API_KEY provided)"
fi

echo ""
echo "════════════════════════════════════════════════════════"
echo " DONE — next steps:"
echo ""
echo "  1. Router: port-forward TCP 80 → 10.0.0.56:80"
echo ""
if [ -n "$IONOS_API_KEY" ]; then
echo "  2. GE Web URL (Layers → Add KML URL):"
echo "     http://${IONOS_SUBDOMAIN}.${IONOS_DOMAIN}/wrecks/networklink.kmz"
echo ""
echo "  3. Web frontend deploy (from your dev machine):"
echo "     cd tauri && npm run build"
echo "     VITE_API_BASE=http://${IONOS_SUBDOMAIN}.${IONOS_DOMAIN} npx wrangler pages deploy dist"
echo ""
echo "  4. Update .env:"
echo "     API_BASE_URL=http://${IONOS_SUBDOMAIN}.${IONOS_DOMAIN}"
else
    PUBLIC_IP=$(curl -s https://api.ipify.org 2>/dev/null || echo "<your-home-public-ip>")
echo "  2. No IONOS key given. Current public IP:"
echo "     http://${PUBLIC_IP}/wrecks/networklink.kmz"
echo ""
echo "  To enable IONOS DDNS for api.cesarops.com, re-run:"
echo "     bash setup_nginx_i7.sh <IONOS_API_KEY>"
echo "  Get your key: https://developer.hosting.ionos.com → Create API Key"
fi
echo "════════════════════════════════════════════════════════"
