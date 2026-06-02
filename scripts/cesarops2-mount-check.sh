#!/usr/bin/env bash
# Verify local + NFS storage mounts on cesarops2.
set -euo pipefail

ok=0
fail=0

check_mount() {
  local mp=$1
  local min_gb=${2:-1}
  if findmnt "$mp" >/dev/null 2>&1; then
    local avail
    avail=$(df -BG "$mp" 2>/dev/null | awk 'NR==2 {gsub(/G/,"",$4); print $4}')
    if [[ "${avail:-0}" -ge "$min_gb" ]]; then
      echo "  OK   $mp (${avail}G free)"
      ok=$((ok + 1))
    else
      echo "  WARN $mp mounted but only ${avail}G free (need >=${min_gb}G)"
      fail=$((fail + 1))
    fi
  else
    echo "  FAIL $mp not mounted"
    fail=$((fail + 1))
  fi
}

echo "[mount-check] cesarops2 storage"
echo "--- local disks (fstab) ---"
check_mount "/" 50
check_mount "/mnt/data-external" 10
check_mount "/data" 0   # legacy 481G — warn if full

data_use=$(df /data 2>/dev/null | awk 'NR==2 {print $5}' | tr -d '%')
if [[ "${data_use:-0}" -ge 95 ]]; then
  echo "  WARN /data is ${data_use}% full — do not write models here; run: bash scripts/cesarops2-disk-cleanup.sh"
  fail=$((fail + 1))
fi

echo "--- NFS (T440, requires 10.0.0.61) ---"
for mp in /mnt/t440/data /mnt/t440/models /mnt/t440/codebase /mnt/t440/repo /mnt/t440/home \
  /mnt/data-external/cesarops /mnt/data-external/projects; do
  if findmnt "$mp" >/dev/null 2>&1; then
    echo "  OK   $mp"
    ok=$((ok + 1))
  else
    echo "  FAIL $mp"
    fail=$((fail + 1))
  fi
done

echo "--- recommended writable roots ---"
source "$(dirname "$0")/cesarops2-storage-paths.sh" 2>/dev/null || true
echo "  primary:  ${CESAROPS_DATA_ROOT:-$HOME/cesarops-data} (on /)"
echo "  HF cache: ${HF_HOME:-$HOME/.cache/huggingface}"
echo "  spill:    /mnt/data-external (local ${SDB_LOCAL_GB:-110}G disk, not NFS subdirs)"

if [[ "${1:-}" == "--remount" ]]; then
  echo "[mount-check] sudo mount -a …"
  sudo mount -a 2>&1 || true
fi

echo "[mount-check] $ok ok, $fail issues"
exit $(( fail > 0 ? 1 : 0 ))
