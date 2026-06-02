#!/usr/bin/env bash
# Write var/fleet-health/latest.json from GPU heartbeats + live probes.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/fleet_resolve.sh
source "${SCRIPT_DIR}/lib/fleet_resolve.sh"
exec python3 "${REPO}/scripts/fleet_health_poll.py" "$@"
