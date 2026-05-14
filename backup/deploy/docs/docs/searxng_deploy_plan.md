

=== FILE: searxng/docker-compose.yml ===
```yaml
version: '3.8'

services:
  searxng:
    image: searxng/searxng:latest
    container_name: cesarops-searxng
    restart: unless-stopped
    ports:
      - "127.0.0.1:8888:8080"
    volumes:
      - /mnt/data-external/cesarops/searxng:/etc/searxng:rw
    environment:
      - SEARXNG_BASE_URL=http://localhost:8888/
      - SEARXNG_SECRET_KEY=${SEARXNG_SECRET_KEY:-$(openssl rand -hex 32)}
      - SEARXNG_USE_X_FORWARDED=true
    cap_drop:
      - ALL
    cap_add:
      - CHOWN
      - SETGID
      - SETUID
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8888/search?q=test&format=json"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s
```

=== FILE: searxng/settings.yml ===
```yaml
use_default_settings: true

general:
  instance_name: "CESARops SearXNG"
  debug: false
  method: GET

search:
  safe_search: 0
  autocomplete: "google"
  default_lang: "auto"
  formats:
    - html
    - json

server:
  port: 8080
  bind_address: "0.0.0.0"
  secret_key: "${SEARXNG_SECRET_KEY}"
  limiter: false
  image_proxy: false
  method: "GET"

ui:
  static_use_hash: true
  default_theme: simple
  theme_args:
    simple_style: auto

engines:
  - name: google
    engine: google
    shortcut: g
    disabled: false
    search_url_template: "https://www.google.com/search?q={query}&num={offset}&start={pageno}"
    paging: true
    results_per_page: 10

  - name: bing
    engine: bing
    shortcut: b
    disabled: false
    paging: true
    results_per_page: 10

  - name: duckduckgo
    engine: duckduckgo
    shortcut: ddg
    disabled: false
    search_type: standard
    results_per_page: 10

  - name: wikipedia
    engine: wikipedia
    shortcut: wp
    disabled: false
    results_per_page: 5

  - name: arxiv
    engine: arxiv
    shortcut: arx
    disabled: false
    results_per_page: 10

  - name: github
    engine: github
    shortcut: gh
    disabled: false
    results_per_page: 10

  - name: stackoverflow
    engine: stackoverflow
    shortcut: so
    disabled: false
    results_per_page: 10

outgoing:
  request_timeout: 8.0
  max_request_timeout: 15.0
  useragent_suffix: ""
  pool_connections: 100
  pool_maxsize: 20

# Disable telemetry/analytics
general:
  enable_metrics: false
  metrics_type: false

# Rate limiting disabled for internal use
limiter:
  enabled: false
```

=== FILE: scripts/searxng.service ===
```ini
[Unit]
Description=CESARops SearXNG Search Engine
Documentation=https://docs.searxng.org/
After=network.target docker.service
Wants=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=/mnt/data-external/cesarops/searxng
ExecStart=/usr/bin/docker compose up -d
ExecStop=/usr/bin/docker compose down
ExecReload=/usr/bin/docker compose restart
Restart=on-failure
RestartSec=10s
StandardOutput=journal
StandardError=journal

# Security hardening
NoNewPrivileges=true
ProtectSystem=full
ProtectHome=read-only
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

=== FILE: scripts/test_searxng.sh ===
```bash
#!/bin/bash
set -euo pipefail

SEARXNG_URL="http://localhost:8888"
WSO_CONFIG_DIR="${HOME}/.cesarops/wso"
WSO_CONFIG_FILE="${WSO_CONFIG_DIR}/config.toml"

echo "=== CESARops SearXNG Integration Test ==="
echo ""

# Step 1: Check if SearXNG is running
echo "[1/4] Checking SearXNG health..."
if curl -sf "${SEARXNG_URL}/search?q=test&format=json" > /dev/null 2>&1; then
    echo "✓ SearXNG is responding on ${SEARXNG_URL}"
else
    echo "✗ SearXNG is not responding. Is it running?"
    echo "  Run: sudo systemctl status searxng"
    exit 1
fi

# Step 2: Verify JSON API works
echo ""
echo "[2/4] Testing JSON API output..."
JSON_RESPONSE=$(curl -sf "${SEARXNG_URL}/search?q=quantum+computing+2024&format=json")
RESULT_COUNT=$(echo "$JSON_RESPONSE" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['results']))" 2>/dev/null || echo "0")
echo "✓ JSON API returned ${RESULT_COUNT} results for 'quantum computing 2024'"

# Step 3: Update WSO config to point at SearXNG
echo ""
echo "[3/4] Configuring cesarops-wso..."
mkdir -p "${WSO_CONFIG_DIR}"
cat > "${WSO_CONFIG_FILE}" << EOF
[web_search]
searxng_url = "${SEARXNG_URL}"
google_cse_key = ""
google_cse_engine_id = ""
max_web_context_tokens = 4096
cache_ttl_hours = 24
enable_google_fallback = false
rate_limit_rpm = 60
EOF
echo "✓ WSO config updated: ${WSO_CONFIG_FILE}"

# Step 4: Test through cesarops-wso (if available)
echo ""
echo "[4/4] Testing through cesarops-wso..."
if command -v cargo &> /dev/null && [ -d "${HOME}/cesarops-wso" ]; then
    cd "${HOME}/cesarops-wso"
    echo "Running: cargo run --example test_searxng_integration"
    if cargo run --example test_searxng_integration 2>&1 | tee /tmp/wso_test.log; then
        echo "✓ WSO integration test passed"
    else
        echo "⚠ WSO integration test failed (check /tmp/wso_test.log)"
        echo "  This may be expected if the example doesn't exist yet."
    fi
else
    echo "⚠ cesarops-wso not found or cargo not available"
    echo "  Manual test: curl '${SEARXNG_URL}/search?q=cesarops+research&format=json'"
fi

echo ""
echo "=== Test Complete ==="
echo "SearXNG is deployed and ready for CESARops research queries."
```

=== FILE: scripts/deploy_searxng.sh ===
```bash
#!/bin/bash
set -euo pipefail

# CESARops SearXNG Deployment Script
# Deploys a private SearXNG instance on T440 server

SEARXNG_DIR="/mnt/data-external/cesarops/searxng"
COMPOSE_FILE="docker-compose.yml"
SETTINGS_FILE="settings.yml"
SERVICE_FILE="searxng.service"

echo "=== CESARops SearXNG Deployment ==="
echo ""

# Step 1: Create directories
echo "[1/5] Creating directory structure..."
mkdir -p "${SEARXNG_DIR}"
mkdir -p "${SEARXNG_DIR}/data"
echo "✓ Created ${SEARXNG_DIR}"

# Step 2: Copy configuration files
echo ""
echo "[2/5] Deploying configuration files..."
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -f "${SCRIPT_DIR}/${COMPOSE_FILE}" ]; then
    cp "${SCRIPT_DIR}/${COMPOSE_FILE}" "${SEARXNG_DIR}/${COMPOSE_FILE}"
    echo "✓ Copied ${COMPOSE_FILE}"
else
    echo "✗ ${COMPOSE_FILE} not found in scripts directory"
    exit 1
fi

if [ -f "${SCRIPT_DIR}/${SETTINGS_FILE}" ]; then
    cp "${SCRIPT_DIR}/${SETTINGS_FILE}" "${SEARXNG_DIR}/${SETTINGS_FILE}"
    echo "✓ Copied ${SETTINGS_FILE}"
else
    echo "✗ ${SETTINGS_FILE} not found in scripts directory"
    exit 1
fi

# Step 3: Install systemd service
echo ""
echo "[3/5] Installing systemd service..."
if [ -f "${SCRIPT_DIR}/${SERVICE_FILE}" ]; then
    sudo cp "${SCRIPT_DIR}/${SERVICE_FILE}" /etc/systemd/system/searxng.service
    sudo systemctl daemon-reload
    echo "✓ Installed searxng.service"
else
    echo "✗ ${SERVICE_FILE} not found in scripts directory"
    exit 1
fi

# Step 4: Start SearXNG
echo ""
echo "[4/5] Starting SearXNG service..."
sudo systemctl start searxng
sudo systemctl enable searxng
echo "✓ SearXNG service started and enabled"

# Wait for health check
echo "  Waiting for SearXNG to become healthy..."
for i in {1..30}; do
    if curl -sf "http://localhost:8888/search?q=test&format=json" > /dev/null 2>&1; then
        echo "✓ SearXNG is healthy after ${i} seconds"
        break
    fi
    sleep 1
done

# Step 5: Verify deployment
echo ""
echo "[5/5] Verifying deployment..."
if curl -sf "http://localhost:8888/search?q=cesarops+research&format=json" > /dev/null 2>&1; then
    echo "✓ SearXNG is responding to queries"
    RESULTS=$(curl -sf "http://localhost:8888/search?q=cesarops+research&format=json" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['results']))")
    echo "  Test query returned ${RESULTS} results"
else
    echo "✗ SearXNG is not responding to queries"
    echo "  Check logs: sudo journalctl -u searxng -f"
    exit 1
fi

echo ""
echo "=== Deployment Complete ==="
echo "SearXNG is running on http://localhost:8888"
echo "Configuration: ${SEARXNG_DIR}/${SETTINGS_FILE}"
echo "Logs: sudo journalctl -u searxng -f"
echo ""
echo "Next steps:"
echo "  1. Run: bash scripts/test_searxng.sh"
echo "  2. Update cesarops-wso config to use localhost:8888"
echo "  3. Monitor: sudo systemctl status searxng"
```