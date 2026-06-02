#!/usr/bin/env bash
# Fleet SSH helpers — use ~/.ssh/config aliases (e.g. Host t440) not bare IPs.
# shellcheck disable=SC2034
set -euo pipefail

# Legacy bare IP → ssh config Host (see ~/.ssh/config IdentityFile)
FLEET_SSH_ALIASES=(
  "10.0.0.61:t440"
  "10.0.0.62:t440-62"
  "cesarops@10.0.0.61:t440"
  "cesarops@10.0.0.62:t440-62"
)

fleet_ssh_resolve_target() {
  local target="${1:-}"
  local pair from to
  for pair in "${FLEET_SSH_ALIASES[@]}"; do
    from="${pair%%:*}"
    to="${pair##*:}"
    if [[ "$target" == "$from" ]]; then
      echo "$to"
      return 0
    fi
  done
  echo "$target"
}

# Remote REPO path for a fleet node (catalog / rs-deep over ssh).
fleet_ssh_remote_repo() {
  local node="${1:-t440}"
  local config="${FLEET_CATALOG_CONFIG:-${REPO:-}/config/fleet_catalog_roots.json}"
  python3 -c "
import json, sys
from pathlib import Path
cfg = json.loads(Path('$config').read_text())
n = cfg.get('nodes', {}).get('$node', {})
print(n.get('remote_repo') or '${REPO:-/data/codebase/repos/wreckhunter2000-1}')
" 2>/dev/null || echo "${REPO:-/data/codebase/repos/wreckhunter2000-1}"
}

fleet_ssh() {
  local target resolved
  target="$1"
  shift
  resolved="$(fleet_ssh_resolve_target "$target")"
  ssh -o BatchMode=yes -o ConnectTimeout=15 "$resolved" "$@"
}

fleet_scp() {
  local resolved=""
  local args=()
  for a in "$@"; do
    if [[ "$a" == *:* ]] && [[ "$a" != /* ]]; then
      local host="${a%%:*}"
      local rest="${a#*:}"
      resolved="$(fleet_ssh_resolve_target "$host")"
      args+=("${resolved}:${rest}")
    else
      args+=("$a")
    fi
  done
  scp -o BatchMode=yes "${args[@]}"
}

# Rewrite catalog paths for the host that will open the file (NFS mount aliases).
fleet_ssh_map_path() {
  local path="$1"
  local node="${2:-t440}"
  local config="${FLEET_CATALOG_CONFIG:-${REPO:-}/config/fleet_catalog_roots.json}"
  python3 -c "
import json, sys
from pathlib import Path
cfg = json.loads(Path('$config').read_text())
m = cfg.get('nodes', {}).get('$node', {}).get('path_map') or {}
p = sys.argv[1]
for src, dst in sorted(m.items(), key=lambda x: -len(x[0])):
    if p.startswith(src):
        p = dst + p[len(src):]
        break
print(p)
" "$path" 2>/dev/null || echo "$path"
}
