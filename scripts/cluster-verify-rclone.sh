#!/usr/bin/env bash
# Cross-node rclone + Nomad verification (no Forge). Run from cesarops2 or T440.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
T440="${T440:-10.0.0.61}"
C2="${C2:-10.0.0.201}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
NOMAD_ADDR="${NOMAD_ADDR:-http://${T440}:4646}"

pass() { echo "PASS  $*"; }
fail() { echo "FAIL  $*"; FAILS=$((FAILS + 1)); }
FAILS=0

echo "=== cluster-verify-rclone ==="

if curl -sf --max-time 5 "http://${T440}:5001/health" >/dev/null; then
  pass "T440 Gemma :5001 health"
else
  fail "T440 Gemma :5001 health"
fi

if curl -sf --max-time 5 "http://${T440}:5002/health" >/dev/null; then
  pass "T440 Qwen :5002 health"
else
  fail "T440 Qwen :5002 health"
fi

if curl -sf --max-time 5 "${FORGE_URL}/health" >/dev/null; then
  pass "primary Forge ${FORGE_URL}"
else
  fail "primary Forge ${FORGE_URL}"
fi

if [[ -f "${REPO}/infra/nomad/jobs/rclone-state-sync.nomad.hcl" ]]; then
  pass "rclone nomad job HCL present"
else
  fail "rclone nomad job HCL missing"
fi

if command -v nomad >/dev/null && nomad job status rclone-state-sync &>/dev/null; then
  pass "nomad job rclone-state-sync registered"
else
  fail "nomad job rclone-state-sync (set NOMAD_ADDR)"
fi

# Backup tree: only on T440 local disk
if [[ "$(hostname -s)" == "t440cesarops" ]]; then
  if [[ -d /codebase/backups/cesarops-state/state/docs && -d /codebase/backups/cesarops-state/state/infra-nomad ]]; then
    pass "T440 backup state/docs + infra-nomad"
  else
    fail "T440 backup dirs missing (run rclone-setup-t440.sh)"
  fi
else
  echo "SKIP  T440 backup dirs (run ls on T440: /codebase/backups/cesarops-state/state/)"
fi

if grep -q 'cesarops-backup' "${HOME}/.config/rclone/rclone.conf" 2>/dev/null; then
  pass "rclone remote cesarops-backup configured"
elif [[ "$(hostname -s)" == "t440cesarops" ]]; then
  fail "rclone remote not configured on T440"
else
  echo "SKIP  rclone.conf (check on T440)"
fi

echo "=== done (failures=$FAILS) ==="
exit "$FAILS"
