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
