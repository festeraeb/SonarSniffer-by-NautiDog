#!/usr/bin/env bash
set -euo pipefail

# Run a Forge task with retry + tighten loop until score gate passes or attempts exhausted.
#
# Modes:
# - same-model: thinker/coder/reviewer collapsed to one endpoint (default :5200)
# - specialized: coder endpoint first (default :5001), reviewer fallback (:5002)
#
# Usage:
#   bash scripts/forge_run_until_green.sh \
#     --label P1-shell-errors \
#     --task "Fix n8n executeCommand shell errors..." \
#     --mode specialized \
#     --max-attempts 5

FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
MODE="same-model"
MAX_ATTEMPTS=5
LABEL="task"
TASK=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --label) LABEL="$2"; shift 2 ;;
    --task) TASK="$2"; shift 2 ;;
    --mode) MODE="$2"; shift 2 ;;
    --max-attempts) MAX_ATTEMPTS="$2"; shift 2 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

if [[ -z "$TASK" ]]; then
  echo "--task is required"
  exit 1
fi

case "$MODE" in
  same-model)
    PRIMARY_ENDPOINT="${PRIMARY_ENDPOINT:-http://127.0.0.1:5200}"
    SECONDARY_ENDPOINT="${SECONDARY_ENDPOINT:-http://10.0.0.201:5200}"
    ;;
  specialized)
    PRIMARY_ENDPOINT="${PRIMARY_ENDPOINT:-http://127.0.0.1:5001}"
    SECONDARY_ENDPOINT="${SECONDARY_ENDPOINT:-http://127.0.0.1:5002}"
    ;;
  *)
    echo "Invalid --mode: $MODE"
    exit 1
    ;;
esac

OUT_DIR="/tmp/forge_run_until_green_${LABEL}_$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$OUT_DIR"

PROMPT_BASE="$TASK"
FEEDBACK_CONTEXT=""

echo "[forge-run] label=$LABEL mode=$MODE out=$OUT_DIR"

for ((attempt=1; attempt<=MAX_ATTEMPTS; attempt++)); do
  ENDPOINT="$PRIMARY_ENDPOINT"
  if [[ $attempt -gt 1 && -n "$SECONDARY_ENDPOINT" ]]; then
    ENDPOINT="$SECONDARY_ENDPOINT"
  fi

  PROMPT=$(cat <<EOF
$PROMPT_BASE

Contract (must follow):
1. Return exact file edits and exact shell commands.
2. Include backup + rollback commands.
3. Include verification commands (compile/test/health).
4. Keep output concise and executable.
$FEEDBACK_CONTEXT
EOF
)

  cat > "$OUT_DIR/request_attempt_${attempt}.json" <<EOF
{
  "suite": "custom",
  "baseline_id": "interactive_fast",
  "endpoint": "$ENDPOINT",
  "tasks": [
    {
      "label": "$LABEL-attempt-$attempt",
      "prompt": $(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$PROMPT")
    }
  ]
}
EOF

  /usr/bin/timeout 150 curl -sS -X POST "$FORGE_URL/cluster/test/dispatch" \
    -H 'Content-Type: application/json' \
    --data-binary @"$OUT_DIR/request_attempt_${attempt}.json" \
    > "$OUT_DIR/response_attempt_${attempt}.json" || true

  python3 - "$OUT_DIR" "$attempt" <<'PY'
import json, pathlib, sys, re
out = pathlib.Path(sys.argv[1])
attempt = int(sys.argv[2])
resp_path = out / f"response_attempt_{attempt}.json"
raw = resp_path.read_text(errors='replace') if resp_path.exists() else ""

result = {
  "attempt": attempt,
  "json_ok": False,
  "endpoint_used": None,
  "error": "no_response",
  "score": 0,
  "status": "Blocked",
  "response_preview": "",
}

try:
  j = json.loads(raw)
  result["json_ok"] = True
  rows = j.get("results", []) if isinstance(j, dict) else []
  row = rows[0] if rows else {}
  agent = row.get("agent", {}) if isinstance(row, dict) else {}
  txt = agent.get("response", "") if isinstance(agent, dict) else ""
  err = agent.get("error") if isinstance(agent, dict) else "invalid_agent"
  endpoint = agent.get("endpoint_used") if isinstance(agent, dict) else None

  correctness = 40 if txt and not err else 10
  actionability = 0
  safety = 0
  clarity = 0

  low = (txt or "").lower()
  if any(k in low for k in ["bash ", "curl ", "systemctl", "sqlite3", "cp "]):
    actionability += 15
  if any(k in low for k in ["1.", "2.", "3.", "step", "verify"]):
    actionability += 15
  if any(k in low for k in ["backup", "rollback", "restore"]):
    safety = 20
  if txt.strip():
    clarity = 10 if len(txt) < 3000 else 7

  score = correctness + actionability + safety + clarity
  status = "Completed" if score >= 80 and not err else ("Partial" if score >= 55 else "Blocked")

  result.update({
    "endpoint_used": endpoint,
    "error": err,
    "score": score,
    "status": status,
    "response_preview": (txt[:500] if txt else ""),
  })
except Exception as e:
  result["error"] = f"parse_error:{e}"

(out / f"grade_attempt_{attempt}.json").write_text(json.dumps(result, indent=2))
print(json.dumps(result))
PY

done

python3 - "$OUT_DIR" <<'PY'
import json, pathlib, sys
out = pathlib.Path(sys.argv[1])
grades = sorted(out.glob("grade_attempt_*.json"), key=lambda p: int(p.stem.split("_")[-1]))
rows = [json.loads(p.read_text()) for p in grades]
best = max(rows, key=lambda r: r.get("score", 0)) if rows else {"status": "Blocked", "score": 0}
summary = {
  "label": out.name,
  "attempts": len(rows),
  "best": best,
  "all": rows,
}
(out / "summary.json").write_text(json.dumps(summary, indent=2))
print(json.dumps(summary, indent=2))
PY
