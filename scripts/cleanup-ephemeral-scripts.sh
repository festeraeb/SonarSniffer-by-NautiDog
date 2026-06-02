#!/usr/bin/env bash
# Remove approved ephemeral scripts only. Default: dry-run.
# Usage: DRY_RUN=0 bash scripts/cleanup-ephemeral-scripts.sh
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
[[ -d /mnt/t440/repo ]] && REPO="/mnt/t440/repo"
DRY_RUN="${DRY_RUN:-1}"
VERDICT="${VERDICT:-${REPO}/var/script-inventory/latest/forge-verdict.json}"
LOG="${LOG:-${REPO}/var/log/script-cleanup.log}"
MAX_VERDICT_AGE_DAYS="${MAX_VERDICT_AGE_DAYS:-7}"

mkdir -p "$(dirname "$LOG")"
log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" | tee -a "$LOG"; }

is_ephemeral_file() {
  local f="$1"
  [[ "$f" == *"/scripts/_ephemeral/"* ]] && return 0
  head -n 5 "$f" 2>/dev/null | grep -q 'CESAROPS_EPHEMERAL=1' && return 0
  return 1
}

in_verdict_ephemeral() {
  local rel="$1"
  [[ -f "$VERDICT" ]] || return 1
  python3 - "$VERDICT" "$rel" <<'PY'
import json, sys
v, rel = sys.argv[1], sys.argv[2]
data = json.load(open(v))
for row in data.get("scripts", data.get("verdicts", [])):
    p = row.get("path", "")
    if p == rel and row.get("tier") == "ephemeral":
        sys.exit(0)
sys.exit(1)
PY
}

deleted=0
skipped=0

if [[ -f "$VERDICT" ]]; then
  age_days=$(( ( $(date +%s) - $(stat -c %Y "$VERDICT") ) / 86400 ))
  if [[ "$age_days" -gt "$MAX_VERDICT_AGE_DAYS" ]]; then
    log "SKIP: verdict older than ${MAX_VERDICT_AGE_DAYS}d ($VERDICT)"
    exit 2
  fi
fi

# Explicit list from verdict
if [[ -f "$VERDICT" ]]; then
  mapfile -t TARGETS < <(python3 - "$VERDICT" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
for row in data.get("scripts", data.get("verdicts", [])):
    if row.get("tier") == "ephemeral":
        print(row["path"])
PY
)
else
  TARGETS=()
  log "WARN: no forge-verdict.json — only _ephemeral/ and tagged headers"
  mapfile -t TARGETS < <(find "${REPO}/scripts/_ephemeral" -name '*.sh' -type f 2>/dev/null || true)
fi

for rel in "${TARGETS[@]}"; do
  [[ -z "$rel" ]] && continue
  f="${REPO}/${rel#${REPO}/}"
  [[ -f "$f" ]] || { log "missing: $rel"; ((skipped++)) || true; continue }
  if ! is_ephemeral_file "$f" && ! in_verdict_ephemeral "$rel"; then
    log "SKIP (not tagged ephemeral): $rel"
    ((skipped++)) || true
    continue
  fi
  case "$rel" in
    infra/*|systemd/*|scripts/lib/*|cesarops-forge-v2/*)
      log "SKIP (protected prefix): $rel"
      ((skipped++)) || true
      continue
      ;;
  esac
  if [[ "$DRY_RUN" == "1" ]]; then
    log "DRY-RUN delete: $rel"
  else
    rm -f "$f"
    log "DELETED: $rel"
  fi
  ((deleted++)) || true
done

log "done dry_run=$DRY_RUN deleted=$deleted skipped=$skipped"
exit 0
