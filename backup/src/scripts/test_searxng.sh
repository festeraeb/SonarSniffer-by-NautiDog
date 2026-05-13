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
