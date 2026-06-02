#!/usr/bin/env bash
# Wait for Cake Qwen2.5-72B (:8081), run inference-engine fix audit, save staged output.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

CAKE_API="${CAKE_API:-http://127.0.0.1:8081}"
PROMPT_FILE="${PROMPT_FILE:-${REPO}/cesarops-inference/fleet_prompts/CAKE_QWEN72B_INFERENCE_ENGINE_FIX.md}"
STAGED="${REPO}/research_log/staged_fixes"
METRICS="${HOME}/.cache/cesarops/audit_runs.jsonl"
WAIT_SECS="${WAIT_SECS:-7200}"
MAX_TOKENS="${MAX_TOKENS:-8192}"
TEMPERATURE="${TEMPERATURE:-0.15}"

log() { echo "[cake-inference-fix] $*" | tee -a "${CAKE_LOG:-$HOME/.cache/cesarops/cake_fleet.log}"; }

if [[ ! -f "$PROMPT_FILE" ]]; then
  log "missing prompt: $PROMPT_FILE"
  exit 1
fi

# Extract system + user blocks from the markdown brief
read_prompt_section() {
  local tag="$1"
  awk -v tag="$tag" '
    $0 ~ "^## " tag " message" { capture=1; next }
    capture && /^## / { exit }
    capture && /^---$/ { exit }
    capture { print }
  ' "$PROMPT_FILE" | sed '/^$/d' | head -c 24000
}

SYSTEM_MSG="$(read_prompt_section System)"
USER_MSG="$(read_prompt_section User)"

if [[ -z "$SYSTEM_MSG" || -z "$USER_MSG" ]]; then
  log "could not parse System/User sections from $PROMPT_FILE"
  exit 1
fi

log "waiting for Cake API at ${CAKE_API}/v1/models (max ${WAIT_SECS}s)…"
deadline=$((SECONDS + WAIT_SECS))
until curl -sf --max-time 8 "${CAKE_API}/v1/models" >/dev/null; do
  if (( SECONDS >= deadline )); then
    log "timeout — Cake not ready. tail: ${CAKE_LOG:-~/.cache/cesarops/cake_fleet.log}"
    exit 1
  fi
  sleep 15
done
log "Cake API online"

mkdir -p "$STAGED"
ts=$(date -u +%Y%m%dT%H%M%SZ)
out_md="${STAGED}/inference_fix_${ts}.md"
out_json="${STAGED}/inference_fix_${ts}.json"

export SYSTEM_FILE USER_FILE
SYSTEM_FILE="$(mktemp)" USER_FILE="$(mktemp)"
printf '%s' "$SYSTEM_MSG" >"$SYSTEM_FILE"
printf '%s' "$USER_MSG" >"$USER_FILE"
export CAKE_MODEL="${CAKE_MODEL_70B:-Qwen/Qwen2.5-72B-Instruct}"
export MAX_TOKENS TEMPERATURE

payload=$(python3 <<'PY'
import json, os
system = open(os.environ["SYSTEM_FILE"]).read()
user = open(os.environ["USER_FILE"]).read()
print(json.dumps({
  "model": os.environ.get("CAKE_MODEL", "Qwen/Qwen2.5-72B-Instruct"),
  "messages": [
    {"role": "system", "content": system},
    {"role": "user", "content": user},
  ],
  "max_tokens": int(os.environ.get("MAX_TOKENS", "8192")),
  "temperature": float(os.environ.get("TEMPERATURE", "0.15")),
}))
PY
)

log "calling ${CAKE_API}/v1/chat/completions (max_tokens=${MAX_TOKENS})…"
http_code=$(curl -sf --max-time 3600 -w '%{http_code}' -o "$out_json" \
  -X POST "${CAKE_API}/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d "$payload" || echo "000")

rm -f "$SYSTEM_FILE" "$USER_FILE"

if [[ "$http_code" != "200" ]]; then
  log "HTTP ${http_code} — response in ${out_json}"
  exit 1
fi

python3 <<PY >"$out_md"
import json, pathlib
p = pathlib.Path("$out_json")
data = json.loads(p.read_text())
content = data.get("choices", [{}])[0].get("message", {}).get("content", "")
pathlib.Path("$out_md").write_text(content + "\n", encoding="utf-8")
print(f"wrote {len(content)} chars")
PY

echo "{\"ts\":\"${ts}\",\"mode\":\"cake_qwen72b_inference_fix\",\"api\":\"${CAKE_API}\",\"out_md\":\"${out_md}\"}" >>"$METRICS"
log "done → ${out_md}"
log "review: less ${out_md} | head -80 ${out_md}"
