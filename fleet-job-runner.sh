#!/usr/bin/env bash
# Deprecated wrapper — use: bash scripts/fleet-job-runner.sh  OR  fleet jobs
exec "$(dirname "$0")/scripts/fleet-job-runner.sh" "$@"
