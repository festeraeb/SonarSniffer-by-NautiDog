#!/usr/bin/env bash
# Unified fleet mode — one cluster, no cesarops2 isolation block on peer dispatch.
# Source after fleet_resolve.sh
set -euo pipefail

FLEET_UNIFIED_MARK="${FLEET_UNIFIED_MARK:-${HOME}/.cache/cesarops/fleet-unified}"
FLEET_ISOLATION_MARK="${FLEET_ISOLATION_MARK:-${HOME}/.cache/cesarops/cesarops2-isolated}"
T440_LAN="${T440_LAN:-10.0.0.61}"
C2_LAN="${C2_LAN:-10.0.0.201}"

fleet_unified_enabled() {
  [[ "${FLEET_UNIFIED:-0}" == "1" || -f "$FLEET_UNIFIED_MARK" ]]
}

fleet_unified_on() {
  mkdir -p "$(dirname "$FLEET_UNIFIED_MARK")"
  touch "$FLEET_UNIFIED_MARK"
  rm -f "$FLEET_ISOLATION_MARK"
  export FLEET_UNIFIED=1
  export ALLOW_T440_FLEET=1
  export CESAROPS2_ISOLATED=0
}

fleet_unified_off() {
  rm -f "$FLEET_UNIFIED_MARK"
  unset FLEET_UNIFIED ALLOW_T440_FLEET 2>/dev/null || true
}

fleet_n8n_primary_url() {
  echo "http://${T440_LAN}:5678"
}

fleet_n8n_local_url() {
  echo "http://127.0.0.1:5678"
}

# Effective URL for probes: primary T440 if up, else local bridge on c2.
fleet_n8n_probe_url() {
  local primary local
  primary="$(fleet_n8n_primary_url)"
  local="$(fleet_n8n_local_url)"
  if curl -sf --max-time 3 "${primary}/healthz" >/dev/null 2>&1; then
    echo "$primary"
  elif curl -sf --max-time 3 "${local}/healthz" >/dev/null 2>&1; then
    echo "$local"
  else
    echo "$primary"
  fi
}

probe_http() {
  local name="$1" url="$2"
  local code
  code=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 4 "$url" 2>/dev/null || echo "000")
  if [[ "$code" =~ ^[23] ]]; then
    echo "  ok   $name  $url  ($code)"
  else
    echo "  DOWN $name  $url  ($code)"
  fi
}

probe_mcp_stack() {
  local cfg="${REPO}/cesarops-forge-v2/cluster_config.toml"
  local c7="${CONTEXT7_URL:-http://127.0.0.1:3737}"
  local c4="${CRAWL4AI_URL:-http://127.0.0.1:11235}"
  local om="${OPENMEMORY_URL:-http://127.0.0.1:8765}"
  local mcp="${MCP_WORKER_URL:-http://127.0.0.1:8090}"
  [[ -f "$cfg" ]] && command -v python3 >/dev/null 2>&1 && {
    c7=$(python3 -c "import re; t=open('$cfg').read(); m=re.search(r'context7_url\s*=\s*\"([^\"]+)\"',t); print(m.group(1) if m else '$c7')" 2>/dev/null || echo "$c7")
    c4=$(python3 -c "import re; t=open('$cfg').read(); m=re.search(r'crawl4ai_url\s*=\s*\"([^\"]+)\"',t); print(m.group(1) if m else '$c4')" 2>/dev/null || echo "$c4")
    om=$(python3 -c "import re; t=open('$cfg').read(); m=re.search(r'openmemory_url\s*=\s*\"([^\"]+)\"',t); print(m.group(1) if m else '$om')" 2>/dev/null || echo "$om")
  }
  echo "--- mcp stack ---"
  probe_http "context7" "${c7}/"
  probe_http "crawl4ai" "${c4}/health"
  probe_http "openmemory" "${om}/"
  probe_http "mcp_worker" "${mcp}/health"
}

run_t440_queue_from_c2() {
  [[ "${FLEET_NODE:-}" == "cesarops2" ]] || return 0
  local q="${REPO}/var/fleet-jobs/pending/t440"
  local n
  n=$(find "$q" -maxdepth 1 -name '*.json' 2>/dev/null | wc -tr -d ' ')
  [[ "${n:-0}" -gt 0 ]] || return 0
  log_peer() { echo "[fleet-unified] $*"; }
  log_peer "draining ${n} T440 queue job(s) from cesarops2 (NFS recovery)"
  FLEET_NODE=t440 REPO="${REPO}" ALLOW_T440_FLEET=1 \
    bash "${REPO}/scripts/fleet-job-runner.sh" || log_peer "warn: T440 queue drain had failures"
}
