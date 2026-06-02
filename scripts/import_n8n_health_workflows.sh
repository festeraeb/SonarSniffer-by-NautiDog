#!/usr/bin/env bash
# Import + auto-activate fleet health / forge watchdog workflows.
set -euo pipefail

export PATH="/home/cesarops/node-v20.18.0-linux-x64/bin:${PATH}"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
N8N_PORT="${N8N_PORT:-5678}"
N8N_DIR="${N8N_DIR:-}"
NODE_BIN="${NODE_BIN:-$(command -v node || true)}"
N8N_DB="${N8N_DB:-}"

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

bash "${REPO}/scripts/start_n8n.sh"

cd "$N8N_DIR"
export N8N_USER_FOLDER="${N8N_USER_FOLDER:-/mnt/t440/data/n8n_data}"
[[ -f "${N8N_USER_FOLDER}/database.sqlite" ]] || export N8N_USER_FOLDER="${HOME}/.n8n"
for wf in \
  "${REPO}/missions/n8n_fleet_ops_dispatch.json" \
  "${REPO}/missions/n8n_fleet_route_health.json" \
  "${REPO}/missions/n8n_forge_health.json" \
  "${REPO}/n8n_moe_tool_router.json" \
  "${REPO}/missions/n8n_llm_output_translator.json" \
  "${REPO}/n8n_llm_output_translator.json" \
  "${REPO}/missions/n8n_predictive_async_moe.json" \
  "${REPO}/missions/n8n_prompt_tuner_worker.json" \
  "${REPO}/scripts/n8n_cluster_health.json"; do
  [[ -f "$wf" ]] || continue
  echo "Importing $(basename "$wf")…"
  n8n_cli import:workflow --input="$wf" || true
done

echo "=== Auto-activate workflows ==="
if [[ -n "$N8N_DB" ]]; then
  N8N_DB="$N8N_DB" bash "${REPO}/scripts/n8n_activate_fleet_workflows.sh"
else
  bash "${REPO}/scripts/n8n_activate_fleet_workflows.sh"
fi

echo "=== Fleet job queue permissions (NFS recovery) ==="
FJ="${REPO}/var/fleet-jobs"
mkdir -p "${FJ}/pending/t440" "${FJ}/pending/cesarops2" "${FJ}/running/t440" "${FJ}/running/cesarops2" \
  "${FJ}/done/t440" "${FJ}/done/cesarops2" "${FJ}/failed/t440" "${FJ}/failed/cesarops2"
chmod -R ugo+rwX "${FJ}" 2>/dev/null || chmod -R a+rwX "${FJ}" 2>/dev/null || true

echo "=== Install host n8n watchdog ==="
echo "  sudo cp ${REPO}/systemd/cesarops-n8n-watchdog.* /etc/systemd/system/"
echo "  sudo systemctl daemon-reload && sudo systemctl enable --now cesarops-n8n-watchdog.timer"
echo ""
echo "=== cesarops2 recovery setup (run ON 10.0.0.201) ==="
echo "  T440_IP=10.0.0.61 bash ${REPO}/scripts/setup_cesarops2_fleet_recovery.sh"
echo ""
echo "Active health workflows:"
n8n_cli list:workflow --active=true 2>/dev/null | grep -iE 'Fleet|Forge|Route|Prompt|PAMP|MoE' || true
