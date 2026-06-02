#!/usr/bin/env bash
# Run ON cesarops2 (10.0.0.201): mount T440 repo RW + expose fleet-jobs queue for T440 recovery.
#
# When T440 is broken, cesarops2 can:
#   - Read/write /mnt/t440/repo/var/fleet-jobs/pending/t440/*.json
#   - Run: FLEET_NODE=t440 bash .../fleet-job-runner.sh
#   - Enqueue: FLEET_DISPATCH=nfs bash .../fleet-n8n-dispatch.sh t440 forge_full_recovery
#
# Usage: T440_IP=10.0.0.61 bash scripts/setup_cesarops2_fleet_recovery.sh
set -euo pipefail

T440="${T440_IP:-10.0.0.61}"
REPO_LOCAL="${CESAROPS_REPO:-/mnt/t440/repo}"
REPO="${REPO_LOCAL}"
OPTS="rw,relatime,vers=4.2,rsize=1048576,wsize=1048576,hard,timeo=600,retrans=2"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_CANON="${REPO_CANON:-/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO_CANON" ]] && INSTALL_SRC="$REPO_CANON" || INSTALL_SRC="$SCRIPT_DIR/.."

mount_nfs() {
  local remote="$1" local_mnt="$2"
  sudo mkdir -p "$local_mnt"
  if mountpoint -q "$local_mnt" 2>/dev/null; then
    echo "  ok: $local_mnt"
    return 0
  fi
  sudo mount -t nfs4 "${T440}:${remote}" "$local_mnt" -o "$OPTS" \
    && echo "  mounted: $local_mnt" \
    || { echo "  FAILED: ${T440}:${remote} -> $local_mnt"; return 1; }
}

echo "=== cesarops2 fleet recovery ← T440 (${T440}) ==="

echo "[1/5] NFS mounts (repo + data)"
mount_nfs /codebase/repos/wreckhunter2000-1 "$REPO"
mount_nfs /data /mnt/t440/data 2>/dev/null || true
mount_nfs /codebase /mnt/t440/codebase 2>/dev/null || true

echo "[2/5] Fleet job queue (shared RW)"
FJ="${REPO}/var/fleet-jobs"
for sub in pending running done failed; do
  for node in t440 cesarops2; do
    mkdir -p "${FJ}/${sub}/${node}"
  done
done
chmod -R ugo+rwX "${FJ}" 2>/dev/null || chmod -R a+rwX "${FJ}" 2>/dev/null || true
echo "  queue: ${FJ}"
ls -la "${FJ}/pending/" 2>/dev/null || true

echo "[3/5] Symlink for scripts that expect /mnt/t440/codebase/repos/wreckhunter2000-1"
sudo mkdir -p /mnt/t440/codebase/repos
if [[ ! -e /mnt/t440/codebase/repos/wreckhunter2000-1 ]]; then
  sudo ln -sfn "$REPO" /mnt/t440/codebase/repos/wreckhunter2000-1
fi

echo "[4/5] Install systemd timers (queue runners + mirror + cleanup)"
for unit in \
  cesarops-fleet-job-runner-c2.service \
  cesarops-fleet-job-runner-t440.service \
  cesarops-fleet-job-runner-c2.timer \
  cesarops-fleet-job-runner-t440.timer \
  cesarops-fleet-mirror.service \
  cesarops-fleet-mirror.timer \
  cesarops-fleet-cleanup.service \
  cesarops-fleet-cleanup.timer; do
  if [[ -f "${INSTALL_SRC}/systemd/${unit}" ]]; then
    sudo cp "${INSTALL_SRC}/systemd/${unit}" /etc/systemd/system/
  fi
done
sudo systemctl daemon-reload
sudo systemctl enable --now cesarops-fleet-job-runner-c2.timer 2>/dev/null || true
sudo systemctl enable --now cesarops-fleet-job-runner-t440.timer 2>/dev/null || true
sudo systemctl enable --now cesarops-fleet-mirror.timer 2>/dev/null || true
sudo systemctl enable --now cesarops-fleet-cleanup.timer 2>/dev/null || true

echo "[5/5] Routes cheat sheet"
cat <<EOF

=== n8n webhooks (host: T440 ${T440}:5678) ===
  fleet-ops:     http://${T440}:5678/webhook/fleet-ops
  tool-route:    http://${T440}:5678/webhook/tool-route
  prompt-tuner:  http://${T440}:5678/webhook/prompt-tuner-initial

=== When T440 n8n is down — NFS queue from cesarops2 ===
  export FLEET_DISPATCH=nfs
  export REPO=${REPO}
  bash ${REPO}/scripts/fleet-n8n-dispatch.sh t440 forge_full_recovery
  bash ${REPO}/scripts/fleet-n8n-dispatch.sh t440 restart_forge
  FLEET_NODE=t440 bash ${REPO}/scripts/fleet-job-runner.sh

=== Forge health probe (run on whoever can reach Forge) ===
  FORGE_URL=http://${T440}:9100 bash ${REPO}/scripts/forge-health-probe.sh
  cat /tmp/forge_health_last.json

=== Queue paths on this host ===
  pending t440:     ${FJ}/pending/t440/
  pending cesarops2: ${FJ}/pending/cesarops2/

EOF
