#!/usr/bin/env bash
# Start n8n on T440 (Node 20, port 5678). Idempotent.
set -euo pipefail

export PATH="/home/cesarops/node-v20.18.0-linux-x64/bin:${PATH}"
export N8N_HOST="${N8N_HOST:-0.0.0.0}"
export N8N_PORT="${N8N_PORT:-5678}"
export N8N_PROTOCOL="${N8N_PROTOCOL:-http}"
export WEBHOOK_URL="${WEBHOOK_URL:-http://127.0.0.1:5678/}"

# Fleet DB lives on NFS; keep runtime + activate script on the same sqlite.
if [[ -z "${N8N_USER_FOLDER:-}" ]]; then
  for d in /mnt/t440/data/n8n_data /data/n8n_data; do
    if [[ -f "$d/database.sqlite" ]]; then
      export N8N_USER_FOLDER="$d"
      break
    fi
  done
fi
export N8N_USER_FOLDER="${N8N_USER_FOLDER:-$HOME/.n8n}"
export REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/scripts" ]] || REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"
export REPO

# Resolve n8n install dir across local and T440-mounted layouts.
N8N_DIR="${N8N_DIR:-}"
if [[ -z "$N8N_DIR" ]]; then
  for d in /data/n8n /mnt/t440/data/n8n /data/n8n_data /mnt/t440/data/n8n_data; do
    if [[ -x "$d/node_modules/n8n/bin/n8n" ]]; then
      N8N_DIR="$d"
      break
    fi
  done
fi

if [[ -z "$N8N_DIR" ]]; then
  echo "n8n install not found; set N8N_DIR (expected node_modules/n8n/bin/n8n)" >&2
  exit 1
fi

if curl -sf --max-time 2 "http://127.0.0.1:${N8N_PORT}/healthz" >/dev/null; then
  echo "n8n already running on :${N8N_PORT}"
  exit 0
fi

cd "$N8N_DIR"
nohup node node_modules/n8n/bin/n8n start >> /home/cesarops/n8n.log 2>&1 &
echo "n8n starting (pid $!) from $N8N_DIR — log: /home/cesarops/n8n.log"

for _ in $(seq 1 30); do
  if curl -sf --max-time 2 "http://127.0.0.1:${N8N_PORT}/healthz" >/dev/null; then
    echo "n8n ready http://127.0.0.1:${N8N_PORT}"
    exit 0
  fi
  sleep 2
done
echo "n8n failed to become ready — check /home/cesarops/n8n.log" >&2
exit 1
