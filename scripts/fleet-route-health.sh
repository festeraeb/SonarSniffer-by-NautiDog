#!/usr/bin/env bash
# Probe Forge LLM endpoints + n8n webhooks; use endpoint pools to avoid single-point failures.
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/scripts" ]] || REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"
FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
N8N="${N8N_URL:-http://127.0.0.1:5678}"
CESAROPS2="${CESAROPS2_HOST:-10.0.0.201}"
NAUTIVECS="${NAUTIVECS_URL:-http://127.0.0.1:5003}"
HOST_NAME="$(hostname -s 2>/dev/null | tr '[:upper:]' '[:lower:]')"

fail=0
ok_count=0
warn_count=0

check() {
  local name="$1"
  local url="$2"
  local max="${3:-3}"
  if curl -sf --max-time "$max" "$url" >/dev/null 2>&1; then
    echo "OK  $name $url"
    ok_count=$((ok_count+1))
  else
    echo "FAIL $name $url" >&2
    fail=1
  fi
}

check_optional() {
  local name="$1"
  local url="$2"
  local max="${3:-3}"
  if curl -sf --max-time "$max" "$url" >/dev/null 2>&1; then
    echo "OK  $name $url"
    ok_count=$((ok_count+1))
  else
    echo "WARN $name $url" >&2
    warn_count=$((warn_count+1))
  fi
}

check_any() {
  local name="$1"
  shift
  local urls=("$@")
  local u
  for u in "${urls[@]}"; do
    if curl -sf --max-time 4 "$u" >/dev/null 2>&1; then
      echo "OK  $name $u"
      ok_count=$((ok_count+1))
      return 0
    fi
  done
  echo "FAIL $name none of ${urls[*]}" >&2
  fail=1
  return 1
}

check_models() {
  local name="$1"
  local base="$2"
  local url="${base%/}/v1/models"
  if curl -sf --max-time 4 "$url" >/dev/null 2>&1; then
    echo "OK  $name $url"
    ok_count=$((ok_count+1))
  else
    echo "FAIL $name $url" >&2
    fail=1
  fi
}

check_any_models() {
  local name="$1"
  shift
  local bases=("$@")
  local base url
  for base in "${bases[@]}"; do
    url="${base%/}/v1/models"
    if curl -sf --max-time 4 "$url" >/dev/null 2>&1; then
      echo "OK  $name $url"
      ok_count=$((ok_count+1))
      return 0
    fi
  done
  echo "FAIL $name none of ${bases[*]}" >&2
  fail=1
  return 1
}

check_post_json() {
  local name="$1"
  local url="$2"
  local body="$3"
  local code
  code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 -X POST \
    -H 'Content-Type: application/json' \
    -d "$body" "$url" 2>/dev/null || echo "000")
  if [[ "$code" =~ ^[23] ]] || [[ "$code" == "400" ]]; then
    echo "OK  $name $url (http $code)"
    ok_count=$((ok_count+1))
  else
    echo "FAIL $name $url (http $code)" >&2
    fail=1
  fi
}

check "n8n" "${N8N}/healthz"
check "forge" "${FORGE}/health"
if [[ "$HOST_NAME" == *t440* ]]; then
  check_any "local_llm_pool" \
    "http://127.0.0.1:5001/health" \
    "http://127.0.0.1:5002/health"
  check_any_models "remote_llm_pool" \
    "http://${CESAROPS2}:5200" \
    "http://${CESAROPS2}:5201" \
    "http://${CESAROPS2}:5202"
else
  check_any_models "local_llm_pool" \
    "http://127.0.0.1:5200" \
    "http://127.0.0.1:5201" \
    "http://127.0.0.1:5202"
  check_any_models "remote_llm_pool" \
    "http://${CESAROPS2}:5200" \
    "http://${CESAROPS2}:5201" \
    "http://${CESAROPS2}:5202"
fi
check_models "bootstrap_role" "http://${CESAROPS2}:5201"
check_post_json "nautivecs_query" "${NAUTIVECS%/}/query" '{"query":"forge watchdog probe","top_k":1}'

# Optional service probes used by mission control tooling.
check_optional "mcp_tool_runner" "http://${CESAROPS2}:8090/health"
check_optional "local_ollama" "http://127.0.0.1:11434/api/tags"

# Webhooks must resolve to a live route; 404 means the workflow is missing/inactive.
probe_webhook() {
  local name="$1"
  local url="$2"
  local code
  code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 -X POST \
    -H 'Content-Type: application/json' \
    -d '{"probe":true}' "$url" 2>/dev/null || echo "000")
  if [[ "$code" =~ ^[23] ]] || [[ "$code" == "400" ]]; then
    echo "OK  $name $url (http $code)"
    ok_count=$((ok_count+1))
  else
    echo "FAIL $name $url (http $code)" >&2
    fail=1
  fi
}

probe_webhook "n8n_fleet_ops" "${N8N}/webhook/fleet-ops"
probe_webhook "n8n_tool_route" "${N8N}/webhook/tool-route"
probe_webhook "n8n_llm_translate" "${N8N}/webhook/llm-translate"
probe_webhook "n8n_pamp_route" "${N8N}/webhook/pamp-route"
probe_webhook "n8n_prompt_tuner_initial" "${N8N}/webhook/prompt-tuner-initial"
probe_webhook "n8n_prompt_tuner_failed" "${N8N}/webhook/prompt-tuner-failed"

echo "SUMMARY ok=${ok_count} warn=${warn_count} fail=${fail}"

# Unified health state for n8n workers (last heartbeat + live probes → routing)
if [[ -f "${REPO:-/data/codebase/repos/wreckhunter2000-1}/scripts/fleet_health_poll.py" ]]; then
  REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
  FLEET_HEALTH_PATCH_ROUTING="${FLEET_HEALTH_PATCH_ROUTING:-1}" \
    python3 "${REPO}/scripts/fleet_health_poll.py" || true
fi

exit "$fail"
