#!/usr/bin/env bash
# Run a T440-local script from cesarops2 via Nomad (no SSH).
# Usage: cluster-exec-t440.sh t440-remove-forge
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
[[ -d /mnt/t440/repo ]] && REPO="/mnt/t440/repo"
export NOMAD_ADDR="${NOMAD_ADDR:-http://10.0.0.61:4646}"

name="${1:?e.g. t440-remove-forge}"

case "$name" in
  t440-remove-forge)
    job="t440-remove-forge"
    job_hcl="${REPO}/infra/nomad/jobs/t440-remove-forge.nomad.hcl"
    ;;
  *)
    echo "[cluster-exec-t440] unsupported: $name" >&2
    exit 1
    ;;
esac

command -v nomad >/dev/null || { echo "[cluster-exec-t440] nomad CLI missing" >&2; exit 1; }
[[ -f "$job_hcl" ]] || { echo "[cluster-exec-t440] missing $job_hcl" >&2; exit 1; }

echo "[cluster-exec-t440] nomad batch → T440 (${NOMAD_ADDR})"
out=$(nomad job run -detach "$job_hcl" 2>&1) || {
  echo "$out" >&2
  exit 1
}
echo "$out"
eval_id=$(echo "$out" | sed -n 's/^Evaluation ID: *//p' | head -1)
[[ -n "$eval_id" ]] || { echo "[cluster-exec-t440] no evaluation id" >&2; exit 1; }

for _ in $(seq 1 90); do
  st=$(nomad job status "$job" 2>/dev/null | awk '/^Status/ {print $3; exit}')
  case "$st" in
    dead)
      alloc=$(nomad job allocs -t "$job" 2>/dev/null | awk 'NR==2 {print $1}')
      if [[ -n "$alloc" ]]; then
        echo "--- alloc logs ($alloc) ---"
        nomad alloc logs "$alloc" 2>/dev/null || true
        nomad alloc logs -stderr "$alloc" 2>/dev/null || true
      fi
      exit 0
      ;;
    failed)
      nomad job status "$job" 2>/dev/null || true
      exit 1
      ;;
  esac
  sleep 2
done
echo "[cluster-exec-t440] timeout — nomad alloc logs -job $job" >&2
exit 1
