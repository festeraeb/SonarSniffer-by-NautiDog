#!/usr/bin/env bash
# Apply fleet-hetero-20260601 routing to Forge.
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"

curl -sf -X POST "${FORGE_URL}/cluster/routing/preset/fleet-hetero-20260601" \
  -H 'Content-Type: application/json' || {
  echo "Preset not in Forge yet — set routing manually:"
  echo "  thinker   http://10.0.0.201:5200   (Gemma E4B RTX)"
  echo "  coder     http://10.0.0.61:5001    (Gemma MoE P100)"
  echo "  reviewer  http://10.0.0.61:5002    (Qwen3.6 P100)"
  echo "  corrector http://10.0.0.201:5202   (Qwen2.5-Coder 7B 1070)"
  echo "  polisher  http://10.0.0.61:5010     (Coder-Next CPU T440)"
  exit 1
}
echo "Applied preset fleet-hetero-20260601"
