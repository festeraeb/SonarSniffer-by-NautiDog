#!/usr/bin/env bash
# Build stub fixes and dispatch P100 coder/reviewer agents for any remaining polish.
set -euo pipefail

REPO="${CESAROPS_PROJECT_ROOT:-/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
CARGO_TARGET="${CARGO_TARGET_DIR:-/data/cargo-target}"

echo "== Build nautivecs + forge + mcp-worker =="
export CARGO_TARGET_DIR="$CARGO_TARGET"
(cd "$REPO/nautivecs" && cargo build --release 2>&1 | tail -5)
(cd "$REPO/cesarops-forge-v2" && cargo build --release 2>&1 | tail -5)
(cd "$REPO/cesarops-mcp-worker" && cargo build --release 2>&1 | tail -5)

echo "== Deploy forge =="
bash "$REPO/scripts/deploy_forge.sh"

echo "== Restart nautivecs (if systemd unit exists) =="
if systemctl is-active --quiet nautivecs 2>/dev/null; then
  sudo systemctl restart nautivecs || true
elif pgrep -f 'nautivecs.*serve' >/dev/null; then
  pkill -f 'nautivecs.*serve' || true
  sleep 1
  nohup "$REPO/nautivecs/target/release/nautivecs" serve --port 5003 \
    > /tmp/nautivecs.log 2>&1 &
fi

echo "== P100 stack =="
bash "$REPO/scripts/p100_cycle.sh" status || true

dispatch_agent() {
  local role="$1"
  local endpoint="$2"
  local task="$3"
  curl -sf -X POST "$FORGE_URL/agent/run" \
    -H 'Content-Type: application/json' \
    -d "$(jq -n --arg r "$role" --arg e "$endpoint" --arg m "$task" \
      '{role:$r, endpoint:$e, message:$m, max_iterations:8}')" \
    | head -c 4000
  echo
}

CODER_EP="${P100_CODER_URL:-http://127.0.0.1:5001}"
REVIEWER_EP="${P100_REVIEWER_URL:-mtp}"

TASKS=(
  "Review cesarops-detection/vision-workers/ for production readiness; add README with ports and VISION_MODEL_ROOT."
  "Verify orchestrator load_detection_tiles_from_csv handles wreck_targets_all.csv; suggest tests."
  "Scan cesarops-forge-v2/src/main.rs start_all_workers — wire cluster_config workers or document deferral."
)

for t in "${TASKS[@]}"; do
  echo "--- Coder: $t ---"
  dispatch_agent "coder" "$CODER_EP" "$t" || true
  echo "--- Reviewer (MTP): $t ---"
  dispatch_agent "reviewer" "$REVIEWER_EP" "Review this implementation task outcome: $t" || true
done

echo "== Smoke: nautivecs /add =="
curl -sf -X POST "http://127.0.0.1:5003/add" \
  -H 'Content-Type: application/json' \
  -d '{"text":"stub-finish smoke test","tags":"smoke","source":"finish_worthy_stubs"}' \
  && echo " /add OK" || echo " /add skipped (nautivecs not up)"

echo "== Dry-run satellite pipeline =="
DRY_RUN=true bash "$REPO/scripts/resume_satellite_pipeline.sh" || true

echo "== Vision workers (cpu sim) =="
VISION_MODE="${VISION_MODE:-cpu}" bash "$REPO/scripts/start_vision_workers.sh" start || true

echo "== MCP worker :8090 =="
pkill -f 'cesarops-mcp-worker.*8090' 2>/dev/null || true
nohup "$REPO/cesarops-mcp-worker/target/release/cesarops-mcp-worker" \
  --port 8090 --project-root "$REPO" > /tmp/mcp-worker.log 2>&1 &
sleep 1
curl -sf http://127.0.0.1:8090/health | head -c 200 || echo "mcp-worker not up"

echo "Done."
