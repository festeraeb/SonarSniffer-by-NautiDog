#!/usr/bin/env bash
# P100 dual-GPU: manifest-only Rust ports (NOT the cesarops2 / 2060 51-file queue).
set -euo pipefail
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
LOG="${LOG:-/tmp/manifest_rust_dispatch.log}"
export REPO LOG
pkill -f 'run_python_to_rust_dispatch.py' 2>/dev/null || true
: >"$LOG"
echo "=== manifest rust dispatch $(date -Is) ===" | tee -a "$LOG"
exec python3 "$REPO/scripts/integrate/run_manifest_rust_dispatch.py" 2>&1 | tee -a "$LOG"
