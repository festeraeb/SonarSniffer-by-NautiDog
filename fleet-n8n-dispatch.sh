#!/usr/bin/env bash
# Deprecated wrapper — use: bash scripts/fleet-n8n-dispatch.sh  OR  fleet dispatch
exec "$(dirname "$0")/scripts/fleet-n8n-dispatch.sh" "$@"
