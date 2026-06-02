#!/usr/bin/env bash
# One-shot: dual P100 llama (Gemma coder + R1 reviewer) + Forge routing preset.
# Inference fixes live in cluster_config [inference] + cesarops-forge-v2 rebuild.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
FORGE="${FORGE:-$REPO/cesarops-forge-v2}"

echo "[forge-gemma-r1] starting llama on P100#0 (R1) + P100#1 (Gemma)…"
bash "$REPO/scripts/p100_gemma_r1_dual.sh" start

echo "[forge-gemma-r1] applying routing_state + mode_state…"
cp "$FORGE/routing/routing_state.gemma-r1.json" "$FORGE/routing_state.json"

if [[ -x "$FORGE/target/release/cesarops-forge-v2" ]]; then
  echo "[forge-gemma-r1] forge binary present (restart forge service to pick up [inference] code if you rebuilt)"
else
  echo "[forge-gemma-r1] building forge (release)…"
  (cd "$FORGE" && cargo build --release)
fi

if curl -sf --max-time 3 http://127.0.0.1:9100/health >/dev/null 2>&1; then
  curl -sf -X POST "http://127.0.0.1:9100/cluster/routing/preset/p100-gemma-r1" \
    -H 'Content-Type: application/json' \
    -d '{"start_workers":false}' | head -c 500
  echo
else
  echo "[forge-gemma-r1] Forge :9100 not running — routing_state.json updated; start forge and run:"
  echo "  curl -X POST http://127.0.0.1:9100/cluster/routing/preset/p100-gemma-r1 -H 'Content-Type: application/json' -d '{\"start_workers\":false}'"
fi

echo "[forge-gemma-r1] coder :5001 | reviewer/corrector :5002 | preset p100-gemma-r1"
