#!/usr/bin/env bash
# Wait for native-arch cake binaries, then start c2 + T440 workers and master-wait.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
C2_REPO="${C2_REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
SCRIPT_DIR="${REPO}/scripts/cake"
CESAROPS2_HOST="${CESAROPS2_HOST:-10.0.0.201}"
CESAROPS2_USER="${CESAROPS2_USER:-cesarops}"
LOG="${LOG:-/tmp/launch-fleet-after-pascal-build.log}"
MAX_WAIT="${MAX_WAIT:-7200}"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOG"; }

bin_ready() {
  [[ -x /opt/cesarops/cake/bin/cake-sm60 ]]
}

c2_ready() {
  ssh -o ConnectTimeout=10 "${CESAROPS2_USER}@${CESAROPS2_HOST}" \
    '[[ -x /opt/cesarops/cake/bin/cake-sm75 && -x /opt/cesarops/cake/bin/cake-sm61 ]]'
}

: >"$LOG"
log "waiting for cake-sm60 (T440) and cake-sm75/sm61 (c2), max ${MAX_WAIT}s…"
deadline=$((SECONDS + MAX_WAIT))
while (( SECONDS < deadline )); do
  t440=0 c2=0
  bin_ready && t440=1
  c2_ready && c2=1
  if [[ "$t440" == "1" && "$c2" == "1" ]]; then
    log "all binaries ready"
    break
  fi
  log "t440_sm60=$t440 c2_sm75/61=$c2 — sleep 60s"
  sleep 60
done

if ! bin_ready || ! c2_ready; then
  log "timeout — missing binaries; check /tmp/install_cake_pascal*.log"
  exit 1
fi

export SKIP_P106=1 CAKE_DUAL_P100=1 FREE_LLAMA=0
log "starting c2 workers (prep-c2)…"
ssh "${CESAROPS2_USER}@${CESAROPS2_HOST}" "SKIP_P106=1 REPO=${C2_REPO} bash ${C2_REPO}/scripts/cake/prep-c2-hetero-workers.sh" >>"$LOG" 2>&1

log "starting T440 workers (prep-t440)…"
bash "${SCRIPT_DIR}/prep-t440-70b.sh" >>"$LOG" 2>&1

log "master-wait (background)…"
nohup env SKIP_P106=1 CAKE_DUAL_P100=1 bash "${SCRIPT_DIR}/start-master-when-c2-ready.sh" >>"$LOG" 2>&1 &
log "launched master-wait pid=$! — tail $LOG and ~/.cache/cesarops/cake_fleet.log"
