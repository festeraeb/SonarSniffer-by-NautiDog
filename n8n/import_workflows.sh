#!/usr/bin/env bash
# Import the CESAROPS specialist + orchestrator workflows into n8n.
#
# Uses the n8n CLI (`n8n import:workflow`). The workflows are webhook-triggered;
# after import you must ACTIVATE them in the n8n UI (or they only run in test
# mode). Regenerate the tool catalog first so the orchestrator's LLM sees the
# current knobs.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
WF_DIR="$HERE/workflows"

# 1. Refresh the tool catalog from the live binaries (best-effort).
if bash "$HERE/pipeline_runner.sh" catalog > "$HERE/tool_catalog.json" 2>/dev/null; then
  echo "[import] refreshed tool_catalog.json"
else
  echo "[import] WARN: could not refresh tool_catalog.json (binaries built?); keeping existing"
fi

# 2. Locate the n8n CLI.
N8N_BIN="${N8N_BIN:-}"
if [[ -z "$N8N_BIN" ]]; then
  if command -v n8n >/dev/null 2>&1; then
    N8N_BIN="n8n"
  elif [[ -x /data/n8n/node_modules/.bin/n8n ]]; then
    N8N_BIN="/data/n8n/node_modules/.bin/n8n"
  else
    echo "[import] ERROR: n8n CLI not found. Set N8N_BIN=/path/to/n8n" >&2
    exit 1
  fi
fi
echo "[import] using n8n: $N8N_BIN"

# 3. Import each workflow (specialists first, orchestrator last).
for wf in specialist_satellite specialist_aeromag specialist_bag orchestrator; do
  f="$WF_DIR/$wf.json"
  if [[ ! -f "$f" ]]; then
    echo "[import] WARN: missing $f, skipping"
    continue
  fi
  echo "[import] importing $wf ..."
  "$N8N_BIN" import:workflow --input="$f" || {
    echo "[import] ERROR importing $wf" >&2
    exit 1
  }
done

echo "[import] done. Activate the workflows in the n8n UI, then POST to:"
echo "          http://127.0.0.1:5678/webhook/wreck-search"
