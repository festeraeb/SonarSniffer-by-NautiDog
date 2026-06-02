#!/usr/bin/env bash
# Enqueue fleet work via n8n (preferred) or direct NFS queue (fallback).
# Usage: fleet-n8n-dispatch.sh <node> <action> [extra json fields]
#   node: t440 | cesarops2 | cesarops2-fr  (fr aliases to cesarops2 queue path)
#   action: install_cake | cake_worker_start | cake_fleet_start | ...
set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/fleet_resolve.sh
source "${_SCRIPT_DIR}/lib/fleet_resolve.sh"

NODE="${1:?node (t440|cesarops2|cesarops2-fr)}"
[[ "$NODE" == "cesarops2-fr" ]] && NODE="cesarops2"
ACTION="${2:?action}"
shift 2

# Unified mode always allows peer dispatch
[[ -f "${HOME}/.cache/cesarops/fleet-unified" ]] && export ALLOW_T440_FLEET=1 CESAROPS2_ISOLATED=0

# cesarops2 isolation: block accidental T440 enqueue unless operator overrides
if [[ "${ALLOW_T440_FLEET:-0}" != "1" ]]; then
  if [[ "${CESAROPS2_ISOLATED:-0}" == "1" || -f "${HOME}/.cache/cesarops/cesarops2-isolated" ]]; then
    if [[ "$NODE" == "t440" ]]; then
      echo "[fleet-dispatch] blocked: CESAROPS2_ISOLATED=1 (set ALLOW_T440_FLEET=1 to override)" >&2
      exit 0
    fi
  fi
fi

N8N_URL="${N8N_FLEET_OPS_URL:-http://127.0.0.1:5678/webhook/fleet-ops}"
DISPATCH="${FLEET_DISPATCH:-auto}"

payload=$(env NODE="$NODE" ACTION="$ACTION" python3 -c "
import json, os, sys, time
extra = {}
for a in sys.argv:
    if '=' in a:
        k, v = a.split('=', 1)
        extra[k] = v
print(json.dumps({
    'node': os.environ.get('NODE', ''),
    'action': os.environ.get('ACTION', ''),
    'requested_at': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
    **extra
}))
" "$@")

enqueue_nfs() {
  local q="${REPO}/var/fleet-jobs/pending/${NODE}"
  mkdir -p "$q"
  local id
  id="job_$(date -u +%Y%m%dT%H%M%SZ)_$$"
  echo "$payload" >"${q}/${id}.json"
  echo "[fleet-dispatch] queued ${q}/${id}.json"
}

try_n8n() {
  curl -sf --max-time 30 -X POST "$N8N_URL" \
    -H 'Content-Type: application/json' \
    -d "$payload"
}

case "$DISPATCH" in
  nfs) enqueue_nfs ;;
  n8n)
    try_n8n
    ;;
  auto)
    if try_n8n 2>/dev/null; then
      echo ""
    else
      echo "[fleet-dispatch] n8n unreachable, using NFS queue" >&2
      enqueue_nfs
    fi
    ;;
  *)
    echo "FLEET_DISPATCH must be auto|n8n|nfs" >&2
    exit 1
    ;;
esac
