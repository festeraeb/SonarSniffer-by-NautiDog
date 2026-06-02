#!/usr/bin/env bash
# Run ON T440 — remove Forge from this host entirely (service, port :9100, watchdog restarts).
# Does NOT delete shared repo /codebase (NFS); cesarops2 keeps Forge.
set -euo pipefail

MARKER="/etc/cesarops/forge-primary-cesarops2"
UNIT="cesarops-forge-v2.service"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"

if [[ "$(hostname -s)" != "t440cesarops" ]]; then
  echo "[t440-remove-forge] Run on T440 only (hostname=$(hostname -s))"
  exit 1
fi

echo "[t440-remove-forge] Installing permanent marker…"
sudo mkdir -p /etc/cesarops
echo "primary=http://10.0.0.201:9100 removed=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | sudo tee "$MARKER" >/dev/null

echo "[t440-remove-forge] Stopping Forge and blocking restarts…"
sudo systemctl stop "$UNIT" 2>/dev/null || true
sudo systemctl disable "$UNIT" 2>/dev/null || true
sudo systemctl mask "$UNIT" 2>/dev/null || true

for unit in n8n-watchdog.timer n8n-watchdog.service \
  mission-service-watchdog.timer mission-service-watchdog.service \
  forge-sync-state.timer forge-sync-state.service; do
  sudo systemctl stop "$unit" 2>/dev/null || true
  sudo systemctl disable "$unit" 2>/dev/null || true
done
# User timers (no sudo) on T440
systemctl --user stop forge-sync-state.timer 2>/dev/null || true
systemctl --user disable forge-sync-state.timer 2>/dev/null || true

pkill -f 'cesarops-forge-v2' 2>/dev/null || true
sleep 1
sudo fuser -k 9100/tcp 2>/dev/null || true
sleep 1

echo "[t440-remove-forge] Removing systemd unit files…"
for f in \
  /etc/systemd/system/cesarops-forge-v2.service \
  /lib/systemd/system/cesarops-forge-v2.service \
  /etc/systemd/system/multi-user.target.wants/cesarops-forge-v2.service; do
  if [[ -e "$f" ]]; then
    sudo rm -f "$f"
    echo "  removed $f"
  fi
done
sudo systemctl daemon-reload
sudo systemctl reset-failed "$UNIT" 2>/dev/null || true

# Drop stale Nomad forge-api that claimed :9100 (count=0 job is fine)
export NOMAD_ADDR="${NOMAD_ADDR:-http://127.0.0.1:4646}"
if command -v nomad >/dev/null; then
  nomad job stop -purge forge-api 2>/dev/null || true
  nomad job run "${REPO}/infra/nomad/jobs/forge-api.nomad.hcl" 2>/dev/null || true
fi

if ss -tlnp 2>/dev/null | grep -q ':9100 '; then
  echo "[t440-remove-forge] FAIL: :9100 still in use"
  ss -tlnp | grep ':9100' || true
  exit 1
fi

echo "[t440-remove-forge] OK — Forge removed from T440"
echo "[t440-remove-forge] Primary Forge: http://10.0.0.201:9100 (cesarops2)"
echo "[t440-remove-forge] Watchdogs updated in repo — sync repo on T440 if needed"
