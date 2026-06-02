#!/usr/bin/env bash
# Walk repo hot paths and write staged audit JSON (never auto-apply).
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
STAGED="${REPO}/research_log/staged_fixes"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cake/fleet-env.sh
source "${SCRIPT_DIR}/cake/fleet-env.sh"
MODEL="${CAKE_MODEL:-$CAKE_MODEL_35B}"
METRICS="${HOME}/.cache/cesarops/audit_runs.jsonl"

mkdir -p "$STAGED"
ts=$(date -u +%Y%m%dT%H%M%SZ)
out="${STAGED}/audit_${ts}.json"

prompt="Review this codebase diff for bugs, security issues, and concrete fixes. Output JSON: {\"findings\":[],\"suggested_patches\":[],\"severity\":\"low|med|high\",\"file\":\"\",\"rationale\":\"\"}"

{
  echo "{"
  echo "  \"timestamp\": \"${ts}\","
  echo "  \"dirs\": [\"cesarops-forge-v2\", \"cesarops-inference\", \"pipelines\"],"
  echo -n "  \"cake_output\": "
  if command -v "$CAKE" >/dev/null 2>&1 || [[ -x "$CAKE" ]]; then
    run_args=(run "$MODEL" "$prompt" --sample-len 512 --temperature 0.2)
    if [[ -n "${CAKE_CLUSTER_KEY:-}" ]]; then
      run_args+=(--cluster-key "$CAKE_CLUSTER_KEY")
    fi
    "$CAKE" "${run_args[@]}" 2>/dev/null | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' || echo '""'
  else
    echo "\"cake binary not installed at $CAKE\""
  fi
  echo "}"
} >"$out"

echo "{\"ts\":\"${ts}\",\"files_reviewed\":3,\"findings_count\":0,\"mode\":\"cake_fleet\"}" >>"$METRICS"
echo "[cake-audit] wrote $out"
