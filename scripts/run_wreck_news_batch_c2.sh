#!/usr/bin/env bash
# Run resumable historic-news LLM batch on cesarops2 (RTX 2060 @ :5200).
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
cd "$REPO"

export DB_PATH="${DB_PATH:-$REPO/db/wrecks.db}"
export LLM_URL="${LLM_URL:-http://127.0.0.1:5200/v1/chat/completions}"
PYTHON="${PYTHON:-python3}"
BATCH_SIZE="${BATCH_SIZE:-15}"
MAX_BATCHES="${MAX_BATCHES:-0}"

log() { echo "[wreck-news-batch] $*"; }

if ! curl -sf "${LLM_URL%/v1/chat/completions}/v1/models" >/dev/null 2>&1; then
  log "LLM not up on $LLM_URL — start lab first:"
  log "  bash scripts/cesarops2_research_lab.sh start"
  exit 1
fi

ARGS=(--resume --batch-size "$BATCH_SIZE" --save --apply-coords)
[[ "$MAX_BATCHES" != "0" ]] && ARGS+=(--max-batches "$MAX_BATCHES")

log "DB=$DB_PATH LLM=$LLM_URL batch=$BATCH_SIZE"
exec "$PYTHON" scripts/batch_wreck_historic_news.py "${ARGS[@]}" "$@"
