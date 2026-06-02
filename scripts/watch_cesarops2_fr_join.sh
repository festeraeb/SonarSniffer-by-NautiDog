#!/usr/bin/env bash
# On T440: poll Tailscale until cesarops2-fr is online, then deploy + patch Forge.
set -euo pipefail
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
POLL="${POLL:-30}"
log() { echo "[watch-c2-fr] $(date -u +%H:%M:%S) $*" | tee -a /tmp/watch-c2-fr.log; }

log "waiting for Tailscale peer cesarops2-fr…"
while true; do
  IP=$(tailscale status --json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
for v in (d.get('Peer') or {}).values():
    if v.get('HostName') in ('cesarops2-fr','cesarops2-fr-1'):
        if v.get('Online'): print(v['TailscaleIPs'][0]); break
" 2>/dev/null || true)
  if [[ -n "$IP" ]]; then
    log "cesarops2-fr online at $IP — deploying"
    REMOTE="cesarops@${IP}" bash "${REPO}/scripts/deploy_configs_to_cesarops2_fr.sh" && log "deploy OK" || log "deploy failed"
  bash "${REPO}/scripts/patch_cluster_config_cesarops2_fr.sh" "$IP" && log "Forge patched"
    exit 0
  fi
  sleep "$POLL"
done
