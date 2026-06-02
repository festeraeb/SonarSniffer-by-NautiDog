#!/usr/bin/env bash
# Activate fleet / health / tuner n8n workflows by name (idempotent).
set -euo pipefail

export PATH="/home/cesarops/node-v20.18.0-linux-x64/bin:${PATH}"
DB="${N8N_DB:-}"
N8N_DIR="${N8N_DIR:-}"
NODE_BIN="${NODE_BIN:-$(command -v node || true)}"

if [[ -z "$DB" ]]; then
  for d in \
    /mnt/t440/data/n8n_data/database.sqlite \
    /data/n8n_data/database.sqlite \
    /home/cesarops/.n8n/database.sqlite; do
    if [[ -f "$d" ]]; then
      DB="$d"
      break
    fi
  done
fi

if [[ -z "$DB" || ! -f "$DB" ]]; then
  echo "n8n database not found (checked shared + local defaults)" >&2
  exit 1
fi

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

N8N_BIN="${N8N_BIN:-$N8N_DIR/node_modules/n8n/bin/n8n}"
if [[ ! -x "$N8N_BIN" ]]; then
  echo "n8n CLI not executable: $N8N_BIN" >&2
  exit 1
fi
if [[ -z "$NODE_BIN" ]]; then
  echo "node executable not found; set NODE_BIN" >&2
  exit 1
fi

n8n_cli() {
  "$NODE_BIN" "$N8N_BIN" "$@"
}

export N8N_DB="$DB"
python3 <<'PY'
import os
import sqlite3

db = os.environ["N8N_DB"]
families = [
  ("Fleet Ops Dispatch", ["Fleet Ops Dispatch (Coding+Ops+DAS tuned)", "Fleet Ops Dispatch"]),
  ("Fleet Route Health", ["Fleet Route Health"]),
  ("Forge Health", ["Forge Health"]),
  ("MoE Tool Call Router", ["MoE Tool Call Router (Coding+Ops+DAS tuned)", "MoE Tool Call Router"]),
  ("LLM Output Translator (MXFP4 MoE)", ["LLM Output Translator (MXFP4 MoE)", "LLM Output Translator"]),
  ("Predictive Async MoE", ["Predictive Async MoE (PAMP)", "Predictive Async MoE"]),
  ("Prompt Tuner Worker", ["Prompt Tuner Worker (Coding+Ops+DAS tuned)", "Prompt Tuner Worker"]),
]

conn = sqlite3.connect(db)
cur = conn.cursor()
for family, preferred_names in families:
  cur.execute(
    """
    SELECT id, name, active, updatedAt
    FROM workflow_entity
    WHERE (isArchived IS NULL OR isArchived = 0)
      AND name LIKE ?
    ORDER BY updatedAt DESC
    """,
    (f"%{family}%",),
  )
  rows = cur.fetchall()
  if not rows:
    print(f"MISSING  {family}")
    continue

  winner = None
  for preferred in preferred_names:
    for row in rows:
      if row[1] == preferred:
        winner = row
        break
    if winner:
      break
  if winner is None:
    winner = rows[0]

  win_id, win_name, win_active, _ = winner
  cur.execute("UPDATE workflow_entity SET active=1 WHERE id=?", (win_id,))
  print(f"ACTIVE  {win_id}  {win_name}  (was active={win_active})")

  for rid, rname, ractive, _ in rows:
    if rid == win_id:
      continue
    if ractive:
      cur.execute("UPDATE workflow_entity SET active=0 WHERE id=?", (rid,))
      print(f"DEACT  {rid}  {rname}  (family {family})")
conn.commit()
conn.close()
PY

# CLI confirm
while IFS='|' read -r id name; do
  [[ -n "$id" ]] || continue
  n8n_cli update:workflow --id="$id" --active=true 2>/dev/null || true
done < <(n8n_cli list:workflow 2>/dev/null | grep -iE 'Fleet Ops Dispatch|Fleet Route Health|Forge Health|Prompt Tuner Worker|Predictive Async MoE|MoE Tool Call Router|LLM Output Translator' | tail -10)

# n8n only picks up active flag after restart when server is already running.
if curl -sf --max-time 2 "http://127.0.0.1:${N8N_PORT:-5678}/healthz" >/dev/null 2>&1; then
  echo "Restarting n8n so active workflows take effect…"
  pkill -f "node_modules/n8n/bin/n8n" 2>/dev/null || true
  sleep 3
  REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
  bash "${REPO}/scripts/start_n8n.sh"
fi

echo "Active workflows:"
n8n_cli list:workflow 2>/dev/null | grep -iE 'Fleet|Forge|Route|Prompt|PAMP' || true
python3 - "$DB" <<'PY'
import sqlite3
import sys
c = sqlite3.connect(sys.argv[1])
for name, active in c.execute(
    "SELECT name, active FROM workflow_entity WHERE active=1 ORDER BY name"
):
  if any(k in name for k in ("Fleet", "Forge", "Prompt", "PAMP", "Route", "MoE Tool Call Router", "LLM Output Translator")):
        print(f"  active={active}  {name}")
PY
