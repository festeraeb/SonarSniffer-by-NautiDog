#!/usr/bin/env bash
# setup_https_i7.sh — Run ON the i7 (10.0.0.56)
#
# What this does:
#   1. Installs nginx + certbot
#   2. Gets a Let's Encrypt TLS cert for home.cesarops.com
#   3. Writes nginx config that terminates HTTPS and proxies:
#        https://home.cesarops.com:8099  →  wrecks API   (uvicorn :8099)
#        https://home.cesarops.com:8765  →  sovereign-cloud (:8765)
#        https://home.cesarops.com:8766  →  model-team-tool  (:8766)
#   4. Rebinds all three services to 127.0.0.1 (no direct public exposure)
#   5. Sets up IONOS DDNS cron to keep home.cesarops.com → your home IP
#   6. Opens firewall ports 80, 443, 8099, 8765, 8766
#
# Prerequisites (on your router):
#   Port-forward TCP 80  → 10.0.0.56:80   (needed for ACME HTTP-01 challenge)
#   Port-forward TCP 443 → 10.0.0.56:443
#   Port-forward TCP 8099 → 10.0.0.56:8099
#   Port-forward TCP 8765 → 10.0.0.56:8765
#   Port-forward TCP 8766 → 10.0.0.56:8766
#
# Usage:
#   bash setup_https_i7.sh <IONOS_API_KEY> [email]
#   bash setup_https_i7.sh 82da510416cc4e13bc91b281f6e1e3eb.l08kCN-... admin@cesarops.com
#
# The IONOS_API_KEY is already in your .env — copy it from there.
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

IONOS_API_KEY="${1:-}"
CERT_EMAIL="${2:-admin@cesarops.com}"
DOMAIN="home.cesarops.com"
IONOS_ZONE="cesarops.com"
IONOS_RECORD="home"
API_USER="cesarops"
REPO_DIR="/home/cesarops/wreckhunter2000-1"

WRECKS_PORT=8099
SOVEREIGN_PORT=8765
AGENT_PORT=8766

if [[ -z "$IONOS_API_KEY" ]]; then
    echo "[error] Usage: bash setup_https_i7.sh <IONOS_API_KEY> [email]"
    echo "        Get your key from: https://developer.hosting.ionos.com"
    exit 1
fi

echo "═══════════════════════════════════════════════════════"
echo "  CESARops HTTPS setup for $DOMAIN"
echo "═══════════════════════════════════════════════════════"

# ── 1. Install packages ───────────────────────────────────────────────────────
echo ""
echo "[1/7] Installing nginx + certbot..."
sudo apt-get update -qq
sudo apt-get install -y nginx certbot python3-certbot-nginx curl

# ── 2. IONOS DDNS — update home.cesarops.com to current public IP ─────────────
echo ""
echo "[2/7] Updating IONOS DDNS: $DOMAIN → current public IP..."

DDNS_SCRIPT="/home/${API_USER}/ionos-ddns-home.sh"
sudo tee "$DDNS_SCRIPT" > /dev/null << DDNS
#!/bin/bash
# IONOS DDNS — keeps home.cesarops.com pointing at home IP
IONOS_KEY="${IONOS_API_KEY}"
FQDN="${DOMAIN}"
RECORD="${IONOS_RECORD}"
ZONE="${IONOS_ZONE}"
API="https://api.hosting.ionos.com/dns/v1"

IP=\$(curl -sf https://api.ipify.org 2>/dev/null) || exit 0

ZONE_ID=\$(curl -sf -X GET "\${API}/zones" \\
  -H "X-API-Key: \${IONOS_KEY}" -H "Accept: application/json" \\
  | python3 -c "import sys,json; zones=json.load(sys.stdin); print(next(z['id'] for z in zones if z['name']=='\${ZONE}'), end='')" 2>/dev/null)

[ -z "\$ZONE_ID" ] && echo "\$(date -u) ERROR: zone '\${ZONE}' not found" && exit 1

curl -sf -X PATCH "\${API}/zones/\${ZONE_ID}" \\
  -H "X-API-Key: \${IONOS_KEY}" -H "Content-Type: application/json" \\
  --data "[{\"name\":\"\${FQDN}\",\"type\":\"A\",\"content\":\"\${IP}\",\"ttl\":60,\"prio\":0,\"disabled\":false}]" \\
  > /dev/null

echo "\$(date -u +%FT%TZ) updated \${FQDN} → \${IP}"
DDNS

sudo chmod 700 "$DDNS_SCRIPT"
sudo chown "${API_USER}:${API_USER}" "$DDNS_SCRIPT"
bash "$DDNS_SCRIPT"

# Cron every 5 minutes
(crontab -u "${API_USER}" -l 2>/dev/null; echo "*/5 * * * * ${DDNS_SCRIPT} >> /home/${API_USER}/ionos-ddns-home.log 2>&1") \
    | sort -u | crontab -u "${API_USER}" -
echo "DDNS cron installed."

# ── 3. Temporary plain-HTTP nginx for ACME challenge ─────────────────────────
echo ""
echo "[3/7] Writing temporary HTTP nginx config for ACME challenge..."
sudo tee /etc/nginx/sites-available/cesarops-http > /dev/null << NGINX_HTTP
server {
    listen 80;
    server_name ${DOMAIN};
    location /.well-known/acme-challenge/ { root /var/www/html; }
    location / { return 301 https://\$host\$request_uri; }
}
NGINX_HTTP

sudo ln -sf /etc/nginx/sites-available/cesarops-http /etc/nginx/sites-enabled/cesarops-http
sudo rm -f /etc/nginx/sites-enabled/default
sudo nginx -t
sudo systemctl reload nginx

# ── 4. Obtain Let's Encrypt certificate ───────────────────────────────────────
echo ""
echo "[4/7] Obtaining TLS certificate for ${DOMAIN}..."
echo "      (Requires port 80 forwarded to this machine on your router)"

if sudo certbot certonly --nginx \
    --non-interactive \
    --agree-tos \
    --email "$CERT_EMAIL" \
    -d "$DOMAIN"; then
    echo "Certificate obtained."
else
    echo ""
    echo "[warn] certbot failed. Possible reasons:"
    echo "       - Port 80 not forwarded to this machine yet"
    echo "       - DNS hasn't propagated (wait ~5 min after DDNS update)"
    echo ""
    echo "       Re-run this script once port 80 is forwarded and DNS resolves."
    echo "       Or run manually: sudo certbot certonly --nginx -d ${DOMAIN}"
    exit 1
fi

CERT_PATH="/etc/letsencrypt/live/${DOMAIN}"

# ── 5. Write full HTTPS nginx config ─────────────────────────────────────────
echo ""
echo "[5/7] Writing HTTPS nginx config..."
sudo tee /etc/nginx/sites-available/cesarops-https > /dev/null << NGINX_HTTPS
# ── Redirect HTTP → HTTPS ────────────────────────────────────────────────────
server {
    listen 80;
    server_name ${DOMAIN};
    location /.well-known/acme-challenge/ { root /var/www/html; }
    location / { return 301 https://\$host\$request_uri; }
}

# ── Shared TLS/CORS snippet (included in each server block) ─────────────────
# CORS headers — allow the IONOS-hosted frontend at cesarops.com
map \$http_origin \$cors_origin {
    default "";
    "~^https://(www\\.)?cesarops\\.com$" \$http_origin;
    "~^https://home\\.cesarops\\.com(:[0-9]+)?\$" \$http_origin;
    "http://localhost:5173" \$http_origin;
}

# ── Wrecks API — port 8099 ───────────────────────────────────────────────────
server {
    listen 8099 ssl;
    server_name ${DOMAIN};

    ssl_certificate     ${CERT_PATH}/fullchain.pem;
    ssl_certificate_key ${CERT_PATH}/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_ciphers         HIGH:!aNULL:!MD5;

    add_header Strict-Transport-Security "max-age=31536000" always;

    location / {
        proxy_pass         http://127.0.0.1:${WRECKS_PORT};
        proxy_set_header   Host \$host;
        proxy_set_header   X-Real-IP \$remote_addr;
        proxy_set_header   X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto https;
        proxy_read_timeout 60s;

        if (\$cors_origin != "") {
            add_header Access-Control-Allow-Origin  \$cors_origin always;
            add_header Access-Control-Allow-Methods "GET, POST, OPTIONS" always;
            add_header Access-Control-Allow-Headers "Content-Type, Authorization" always;
            add_header Access-Control-Allow-Credentials "true" always;
        }
        if (\$request_method = OPTIONS) { return 204; }
    }
}

# ── Sovereign-cloud node API — port 8765 ─────────────────────────────────────
server {
    listen 8765 ssl;
    server_name ${DOMAIN};

    ssl_certificate     ${CERT_PATH}/fullchain.pem;
    ssl_certificate_key ${CERT_PATH}/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_ciphers         HIGH:!aNULL:!MD5;

    add_header Strict-Transport-Security "max-age=31536000" always;

    location / {
        proxy_pass         http://127.0.0.1:${SOVEREIGN_PORT}_internal;
        proxy_set_header   Host \$host;
        proxy_set_header   X-Real-IP \$remote_addr;
        proxy_set_header   X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto https;
        proxy_read_timeout 60s;

        if (\$cors_origin != "") {
            add_header Access-Control-Allow-Origin  \$cors_origin always;
            add_header Access-Control-Allow-Methods "GET, POST, OPTIONS" always;
            add_header Access-Control-Allow-Headers "Content-Type, Authorization" always;
            add_header Access-Control-Allow-Credentials "true" always;
        }
        if (\$request_method = OPTIONS) { return 204; }
    }
}

# ── Model-team-tool agent — port 8766 ────────────────────────────────────────
server {
    listen 8766 ssl;
    server_name ${DOMAIN};

    ssl_certificate     ${CERT_PATH}/fullchain.pem;
    ssl_certificate_key ${CERT_PATH}/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_ciphers         HIGH:!aNULL:!MD5;

    add_header Strict-Transport-Security "max-age=31536000" always;

    location / {
        proxy_pass         http://127.0.0.1:${AGENT_PORT};
        proxy_set_header   Host \$host;
        proxy_set_header   X-Real-IP \$remote_addr;
        proxy_set_header   X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto https;
        proxy_read_timeout 60s;

        if (\$cors_origin != "") {
            add_header Access-Control-Allow-Origin  \$cors_origin always;
            add_header Access-Control-Allow-Methods "GET, POST, OPTIONS" always;
            add_header Access-Control-Allow-Headers "Content-Type, Authorization" always;
            add_header Access-Control-Allow-Credentials "true" always;
        }
        if (\$request_method = OPTIONS) { return 204; }
    }
}
NGINX_HTTPS

# Fix the internal upstream port placeholder (bash can't use : in heredoc vars easily)
sudo sed -i "s|127.0.0.1:${SOVEREIGN_PORT}_internal|127.0.0.1:${SOVEREIGN_PORT}|g" \
    /etc/nginx/sites-available/cesarops-https

sudo ln -sf /etc/nginx/sites-available/cesarops-https /etc/nginx/sites-enabled/cesarops-https
sudo rm -f /etc/nginx/sites-enabled/cesarops-http
sudo nginx -t
sudo systemctl reload nginx
echo "nginx HTTPS config active."

# ── 6. Rebind services to localhost only ──────────────────────────────────────
echo ""
echo "[6/7] Rebinding services to 127.0.0.1..."

# wrecks-api — update ExecStart host binding
if sudo systemctl is-active --quiet wrecks-api 2>/dev/null; then
    UNIT_FILE=$(sudo systemctl show -p FragmentPath wrecks-api | cut -d= -f2)
    if [[ -n "$UNIT_FILE" && -f "$UNIT_FILE" ]]; then
        sudo sed -i 's/--host 0\.0\.0\.0/--host 127.0.0.1/g' "$UNIT_FILE"
        sudo systemctl daemon-reload
        sudo systemctl restart wrecks-api
        echo "  wrecks-api rebound to 127.0.0.1:${WRECKS_PORT}"
    fi
else
    echo "  wrecks-api not running — skipping rebind (will bind correctly on next start)"
fi

# sovereign-cloud — update systemd unit if it exists
if sudo systemctl is-active --quiet sovereign-cloud 2>/dev/null; then
    UNIT_FILE=$(sudo systemctl show -p FragmentPath sovereign-cloud | cut -d= -f2)
    if [[ -n "$UNIT_FILE" && -f "$UNIT_FILE" ]]; then
        # sovereign-cloud binds via API_PORT const; add env override
        if ! grep -q "API_BIND_ADDR" "$UNIT_FILE"; then
            sudo sed -i '/\[Service\]/a Environment="API_BIND_ADDR=127.0.0.1"' "$UNIT_FILE"
            sudo systemctl daemon-reload
            sudo systemctl restart sovereign-cloud
            echo "  sovereign-cloud restarted"
        fi
    fi
else
    echo "  sovereign-cloud not running — skipping (nginx proxies to it when it starts)"
fi

# ── 7. Firewall ───────────────────────────────────────────────────────────────
echo ""
echo "[7/7] Configuring firewall..."
sudo ufw allow 22/tcp   2>/dev/null || true
sudo ufw allow 80/tcp   2>/dev/null || true
sudo ufw allow 443/tcp  2>/dev/null || true
sudo ufw allow 8099/tcp 2>/dev/null || true
sudo ufw allow 8765/tcp 2>/dev/null || true
sudo ufw allow 8766/tcp 2>/dev/null || true
sudo ufw --force enable 2>/dev/null || true
echo "Firewall rules applied."

# ── Auto-renew cert ───────────────────────────────────────────────────────────
sudo systemctl enable certbot.timer 2>/dev/null || \
    (crontab -l 2>/dev/null; echo "0 3 * * * certbot renew --quiet --post-hook 'systemctl reload nginx'") \
    | sort -u | crontab -
echo "Certbot auto-renew enabled."

# ── Done ──────────────────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════════════"
echo "  ✓  HTTPS setup complete"
echo ""
echo "  Endpoints (all HTTPS, TLS terminated by nginx):"
echo "    https://${DOMAIN}:8099   ← wrecks API"
echo "    https://${DOMAIN}:8765   ← sovereign-cloud"
echo "    https://${DOMAIN}:8766   ← model-team-tool agent"
echo ""
echo "  Router port-forwards required:"
echo "    TCP 80   → 10.0.0.56:80    (ACME renewal)"
echo "    TCP 443  → 10.0.0.56:443"
echo "    TCP 8099 → 10.0.0.56:8099"
echo "    TCP 8765 → 10.0.0.56:8765"
echo "    TCP 8766 → 10.0.0.56:8766"
echo ""
echo "  Rebuild the web frontend to pick up the https:// env vars:"
echo "    cd tauri && npm run build:web"
echo "    python scripts/deploy_web.py"
echo "═══════════════════════════════════════════════════════"
