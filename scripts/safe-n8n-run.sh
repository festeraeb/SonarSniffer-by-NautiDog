#!/usr/bin/env bash
set -euo pipefail

REPO="/codebase/repos/wreckhunter2000-1"
CMD_RAW="${1:-}"
if [[ -z "$CMD_RAW" ]]; then
  echo "missing command" >&2
  exit 2
fi

# Allowed command prefixes for coding + CESAROPS ops.
case "$CMD_RAW" in
  "bash $REPO/scripts/"*|"$REPO/scripts/"*|"cargo "*|"git "*|"ls "*|"cat "*|"grep "*|"rg "*|"curl "*|"python3 "*|"pytest "*|"make "*) ;;
  *)
    echo "command blocked by safe-n8n-run policy: $CMD_RAW" >&2
    exit 13
    ;;
esac

cd "$REPO"
exec bash -lc "$CMD_RAW"