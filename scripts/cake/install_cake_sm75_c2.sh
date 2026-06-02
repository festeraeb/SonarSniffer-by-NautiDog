#!/usr/bin/env bash
# RTX 2060 (sm_75): only Pascal/Turing cake build that compiles (P100/1070 WMMA fails at sm_60/61).
set -euo pipefail
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
C2_REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"
LOG="${LOG:-/tmp/install_cake_sm75_c2.log}"
: >"$LOG"
log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOG"; }

log "sync install scripts to cesarops2…"
scp -q "$REPO/scripts/cake/install_cake_for_cap.sh" "$REPO/scripts/install_cake_fleet.sh" \
  cesarops@10.0.0.201:"$C2_REPO/scripts/cake/" 2>>"$LOG" || true
scp -q "$REPO/scripts/install_cake_fleet.sh" cesarops@10.0.0.201:"$C2_REPO/scripts/" 2>>"$LOG" || true

log "building cake-sm75 on c2 (20–40 min)…"
ssh cesarops@10.0.0.201 "bash $C2_REPO/scripts/cake/install_cake_for_cap.sh 75" >>"$LOG" 2>&1

log "verify PTX arch:"
ssh cesarops@10.0.0.201 'strings /opt/cesarops/cake/bin/cake-sm75 2>/dev/null | grep "^\.target sm_" | sort -u' | tee -a "$LOG"
log "done — $LOG"
