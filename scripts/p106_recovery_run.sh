#!/usr/bin/env bash
# P106 recovery lane — dual-phase file map (rules + optional Jina on GPU).
# Does NOT start llama/PAMP on :5201; uses P106 for PyTorch embedder only.
#
#   bash scripts/p106_recovery_run.sh bootstrap   # clean_repos symlinks
#   bash scripts/p106_recovery_run.sh rules       # fast pass, no GPU models
#   bash scripts/p106_recovery_run.sh full        # Jina on P106 (install deps once)
#
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/scripts" ]] || REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"

export REPO
export RECOVERY_OUT="${RECOVERY_OUT:-$REPO/var/recovery}"
export CLEAN_REPOS="${CLEAN_REPOS:-$REPO/var/recovery/clean_repos}"
if [[ -z "${RECOVERY_SCAN_ROOT:-}" ]]; then
  if [[ -d /data/laptopdump ]]; then
    RECOVERY_SCAN_ROOT=/data/laptopdump
  else
    RECOVERY_SCAN_ROOT=/mnt/t440/data/laptopdump
  fi
fi
export RECOVERY_SCAN_ROOT
# nvidia-smi GPU 0 on cesarops2 = P106-100
export RECOVERY_CUDA_DEVICE="${RECOVERY_CUDA_DEVICE:-0}"
export CUDA_VISIBLE_DEVICES="${CUDA_VISIBLE_DEVICES:-$RECOVERY_CUDA_DEVICE}"

VENV="${RECOVERY_VENV:-$HOME/.venvs/cesarops-recovery}"
PY="${VENV}/bin/python"
MAP="$REPO/scripts/recovery/p106_drive_map.py"

log() { echo "[p106-recovery] $*"; }

ensure_venv() {
  if [[ -x "$PY" ]] && "$PY" -c "import torch, sentence_transformers" 2>/dev/null; then
    return 0
  fi
  log "creating venv at $VENV (torch + sentence-transformers)..."
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install -U pip wheel
  "$VENV/bin/pip" install torch sentence-transformers transformers accelerate
}

cmd_bootstrap() {
  bash "$REPO/scripts/recovery/build_clean_repos.sh"
}

cmd_rules() {
  cmd_bootstrap
  python3 "$MAP" --rules-only --scan "$RECOVERY_SCAN_ROOT" "$@"
}

cmd_full() {
  cmd_bootstrap
  ensure_venv
  "$PY" "$MAP" --full --device "$RECOVERY_CUDA_DEVICE" --scan "$RECOVERY_SCAN_ROOT" "$@"
}

cmd_status() {
  nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv 2>/dev/null || true
  log "scan=$RECOVERY_SCAN_ROOT out=$RECOVERY_OUT"
  [[ -f "$RECOVERY_OUT/manifest_summary.json" ]] && cat "$RECOVERY_OUT/manifest_summary.json" | head -30
}

case "${1:-status}" in
  bootstrap) shift; cmd_bootstrap "$@" ;;
  rules) shift; cmd_rules "$@" ;;
  full) shift; cmd_full "$@" ;;
  status) cmd_status ;;
  *)
    echo "Usage: $0 {bootstrap|rules|full|status} [--max-files N]"
    exit 1
    ;;
esac
