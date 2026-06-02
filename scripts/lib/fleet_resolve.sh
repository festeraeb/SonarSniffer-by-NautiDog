#!/usr/bin/env bash
# Source from fleet scripts: resolves REPO, FLEET_NODE, FORGE_URL, N8N_* from fleet_manifest.json
# Usage: source "${REPO}/scripts/lib/fleet_resolve.sh"   OR   source "$(dirname "$0")/lib/fleet_resolve.sh"
set -euo pipefail

_fleet_resolve_script="${BASH_SOURCE[0]:-$0}"
_fleet_lib_dir="$(cd "$(dirname "$_fleet_resolve_script")" && pwd)"
_fleet_scripts_dir="$(cd "${_fleet_lib_dir}/.." && pwd)"

if [[ -z "${REPO:-}" ]] || [[ ! -f "${REPO}/config/fleet_manifest.json" ]]; then
  for _cand in \
    "/data/codebase/repos/wreckhunter2000-1" \
    "/mnt/t440/codebase/repos/wreckhunter2000-1" \
    "/mnt/t440/repo" \
    "/codebase/repos/wreckhunter2000-1"; do
    if [[ -f "${_cand}/config/fleet_manifest.json" ]]; then
      REPO="${_cand}"
      break
    fi
  done
fi
REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"

_fleet_manifest_py="${REPO}/scripts/fleet_manifest.py"
if [[ -x "${_fleet_manifest_py}" ]] || [[ -f "${_fleet_manifest_py}" ]]; then
  # shellcheck disable=SC1090
  eval "$("$(command -v python3)" "${_fleet_manifest_py}" --repo "${REPO}" env 2>/dev/null)" || true
fi

export REPO
export FLEET_NODE="${FLEET_NODE:-$(python3 "${_fleet_manifest_py}" --repo "${REPO}" host 2>/dev/null || hostname -s | tr '[:upper:]' '[:lower:]')}"
case "$FLEET_NODE" in
  *t440*) FLEET_NODE=t440 ;;
  *) FLEET_NODE=cesarops2 ;;
esac
export FLEET_NODE

export FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
if [[ "$FLEET_NODE" == "t440" && "$REPO" == /mnt/t440/* ]]; then
  export T440_RECOVERY="${T440_RECOVERY:-1}"
  export FORGE_URL="${FORGE_URL:-http://10.0.0.61:9100}"
fi
export N8N_URL="${N8N_URL:-http://127.0.0.1:5678}"
export FLEET_CATALOG_DIR="${FLEET_CATALOG_DIR:-${REPO}/var/fleet-catalog}"
export FLEET_MANIFEST="${FLEET_MANIFEST:-${REPO}/config/fleet_manifest.json}"

_fleet_unified_mark="${HOME}/.cache/cesarops/fleet-unified"
if [[ -f "$_fleet_unified_mark" ]]; then
  export FLEET_UNIFIED=1
  export ALLOW_T440_FLEET=1
  export CESAROPS2_ISOLATED=0
fi

fleet_script() {
  local rel="$1"
  if [[ -f "${REPO}/${rel}" ]]; then
    echo "${REPO}/${rel}"
  else
    echo "${_fleet_scripts_dir}/${rel#scripts/}" >&2
    return 1
  fi
}
