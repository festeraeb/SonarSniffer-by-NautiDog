#!/usr/bin/env bash
# Phase A: TPU + Movidius scan → Phase B: coder round on P100s + RTX + 1070 (no P106).
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
OUT="${OUT:-$REPO/var/role_bench/satel_glint_$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$OUT"

TPU="${TPU_URL:-http://127.0.0.1:8092}"
JITTER="${JITTER_URL:-http://10.0.0.61:8180}"

log() { echo "[satel-glint] $*"; }

log "Phase A — TPU infer @ $TPU"
python3 "$REPO/scripts/role_bench/_satel_phase_a.py" "$OUT" "$TPU" "$JITTER"

log "Phase B — coders (P106 excluded)"
python3 "$REPO/scripts/role_bench/_satel_phase_b.py" "$OUT"

log "Artifacts: $OUT"
ls -la "$OUT"
