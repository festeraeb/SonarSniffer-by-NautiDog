#!/usr/bin/env bash
# Copy Gemini key from Cursor workspace file into repo (after you edit in editor).
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$REPO/scripts/credentials.gemini.local.sh"
SRC="${1:-}"

if [[ -z "$SRC" ]]; then
  for c in \
    "${CURSOR_PROJECT_DIR:-}/credentials.gemini.local.sh" \
    "${HOME}/.cursor/projects/tmp-a596c4f8-e11f-49b5-aca7-49516347f2fe/credentials.gemini.local.sh"; do
    [[ -f "$c" ]] && SRC="$c" && break
  done
fi

if [[ -z "$SRC" || ! -f "$SRC" ]]; then
  echo "Usage: $0 [path/to/credentials.gemini.local.sh]" >&2
  echo "Edit credentials.gemini.local.sh in Cursor workspace root, then run this." >&2
  exit 1
fi

cp "$SRC" "$DEST"
chmod 600 "$DEST"
echo "Synced → $DEST"
grep -q 'GEMINI_API_KEY=".\+"' "$DEST" && echo "Key looks set." || echo "WARN: GEMINI_API_KEY still empty in $DEST"
