#!/usr/bin/env bash
# End-to-end OperatorSpec via Forge API (credentials from credentials.gemini.local.sh).
set -euo pipefail

FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
INTENT="${1:-smoke test: list one safe forge config tweak for corrector repeat cap}"

echo "== POST /spec/draft =="
curl -sS -X POST "$FORGE/spec/draft" \
  -H 'Content-Type: application/json' \
  -d "$(jq -n --arg i "$INTENT" '{intent: $i}')"

echo ""
echo "== POST /spec/approve =="
curl -sS -X POST "$FORGE/spec/approve" \
  -H 'Content-Type: application/json' \
  -d '{"approved":true}'

echo ""
echo "== GET /spec/status =="
curl -sS "$FORGE/spec/status"
echo ""
