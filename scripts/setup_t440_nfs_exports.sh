#!/usr/bin/env bash
# T440 (10.0.0.61): export the PERC arrays for cluster + recovery from cesarops2 (.201).
# Boot stays on the BOSS card and is not exported here.
# When T440 needs repair, work from cesarops2 with NFS mounts — same files, no copy drift.
#
# Run on T440: sudo bash scripts/setup_t440_nfs_exports.sh
set -euo pipefail

T440_IP="${T440_IP:-10.0.0.61}"
T440_TS="${CESAROPS_T440_TS_IP:-100.72.182.77}"
SUBNET="${CESAROPS_LAN:-10.0.0.0/24}"
# Tailscale CGNAT range for off-site laptops (nautik9). Set empty to skip TS exports.
TS_SUBNET="${CESAROPS_TS_SUBNET:-100.64.0.0/10}"
OPTS="rw,sync,no_subtree_check,no_root_squash"
# NFSv4 pseudoroot — required by some clients (WSL2). Only /codebase needs fsid=0.
OPTS_CODEBASE_V4="${OPTS},fsid=0,crossmnt"

# Full recovery surface (prefer fewer top-level mounts on clients)
# RAID10: system/code/data tree.
# RAID0: models + downloads workspace for bulk storage.
EXPORTS=(
  "/data"
  "/codebase"
  "/codebase/repos/wreckhunter2000-1"
  "/mnt/raid0"
  "/home/cesarops"
)
# Note: do NOT export /mnt/data-external alone — it can stack over /dev/sda2
# and hide xfs projects; /codebase is the canonical 465G tree (projects, repos).

echo "[1/5] Symlinks expected by mag/satellite scripts"
MAG_PIPE="/mnt/data-external/projects/pipelines/magnetic_data"
if [[ -d "$MAG_PIPE" ]]; then
  ln -sfn pipelines/magnetic_data /mnt/data-external/projects/magnetic_data
  echo "  projects/magnetic_data -> pipelines/magnetic_data"
fi

echo "[2/5] Install nfs-kernel-server if needed"
if ! command -v exportfs >/dev/null 2>&1; then
  sudo apt-get update -qq
  sudo apt-get install -y nfs-kernel-server
fi

echo "[3/5] Write /etc/exports"
TMP=$(mktemp)
grep -v '^# cesarops cluster' /etc/exports 2>/dev/null \
  | grep -vE '^/(data|mnt/data-external|codebase|home/cesarops|mnt/raid0)' >>"$TMP" || true
{
  cat "$TMP"
  echo ""
  echo "# cesarops cluster — full recovery exports (setup_t440_nfs_exports.sh)"
  for e in "${EXPORTS[@]}"; do
    if [[ ! -e "$e" ]]; then
      echo "# SKIP missing: $e" >&2
      continue
    fi
    if [[ "$e" == "/codebase" ]]; then
      echo "$e $SUBNET($OPTS_CODEBASE_V4)"
      [[ -n "$TS_SUBNET" ]] && echo "$e $TS_SUBNET($OPTS_CODEBASE_V4)"
    else
      echo "$e $SUBNET($OPTS)"
      [[ -n "$TS_SUBNET" ]] && echo "$e $TS_SUBNET($OPTS)"
    fi
  done
} | sudo tee /etc/exports >/dev/null
rm -f "$TMP"

echo "[4/5] exportfs -ra + restart nfs"
sudo exportfs -ra
sudo systemctl enable nfs-server 2>/dev/null || sudo systemctl enable nfs-kernel-server 2>/dev/null || true
sudo systemctl restart nfs-server 2>/dev/null || sudo systemctl restart nfs-kernel-server 2>/dev/null || true

echo "[5/5] Active exports"
sudo exportfs -v 2>/dev/null | grep -E '^/' || true

cat <<EOF

=== Client mount cheatsheet (cesarops2 @ 10.0.0.201) ===

  sudo mkdir -p /mnt/t440/{data,data-external,repo,models,home}
  sudo mount -t nfs4 ${T440_IP}:/data /mnt/t440/data -o rw,hard,timeo=600
  sudo mount -t nfs4 ${T440_IP}:/codebase /mnt/t440/codebase -o rw,hard,timeo=600
  sudo mount -t nfs4 ${T440_IP}:/codebase/repos/wreckhunter2000-1 /mnt/t440/repo -o rw,hard,timeo=600
  sudo mount -t nfs4 ${T440_IP}:/mnt/raid0 /mnt/t440/models -o rw,hard,timeo=600
  sudo mount -t nfs4 ${T440_IP}:/home/cesarops /mnt/t440/home -o rw,hard,timeo=600

Or run on cesarops2: T440_IP=${T440_IP} bash setup_cesarops2_mag_data.sh

=== nautik9 (laptop / WSL) — prefer NFSv4 parent mount ===

  # WSL often fails on nested export; mount /codebase (fsid=0) then use subpath:
  sudo mkdir -p /mnt/t440-codebase
  sudo mount -t nfs4 ${T440_TS}:/codebase /mnt/t440-codebase \\
    -o rw,hard,timeo=600,retrans=2,_netdev,vers=4.2
  sudo mkdir -p /codebase/repos/wreckhunter2000-1
  sudo mount --bind /mnt/t440-codebase/repos/wreckhunter2000-1 /codebase/repos/wreckhunter2000-1

  # If still "Operation not permitted", try NFSv3 on LAN only:
  # sudo mount -t nfs -o vers=3 10.0.0.61:/codebase/repos/wreckhunter2000-1 /codebase/repos/wreckhunter2000-1

  export CESAROPS_CONSUL_ADVERTISE=\$(tailscale ip -4)   # or 100.110.214.86 if TS on Windows only
  cd /codebase/repos/wreckhunter2000-1
  infra/nomad/scripts/require_mounted_repo.sh /codebase/repos/wreckhunter2000-1
  sudo bash infra/nomad/scripts/bootstrap_client.sh nautik9-laptop m2200 --remote

Mag data: /mnt/t440/codebase/projects/pipelines/magnetic_data
Forge:    /mnt/t440/repo/cesarops-forge-v2
Models:   /mnt/t440/models (shared from /mnt/raid0; use for models + downloads)

EOF
