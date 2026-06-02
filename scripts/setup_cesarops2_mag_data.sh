#!/usr/bin/env bash
# Run ON cesarops2 (10.0.0.201) — mount T440 NFS exports for mag + recovery.
# Usage: T440_IP=10.0.0.61 bash setup_cesarops2_mag_data.sh
set -euo pipefail

T440="${T440_IP:-10.0.0.61}"
NODE_IP="${CESAROPS2_IP:-10.0.0.201}"
OPTS="rw,relatime,vers=4.2,rsize=1048576,wsize=1048576,hard,timeo=600,retrans=2"
OPTS_RO="ro,relatime,vers=4.2,rsize=1048576,wsize=1048576,hard,timeo=600,retrans=2"

mount_nfs() {
  local remote="$1" local_mnt="$2" mopts="${3:-$OPTS}"
  sudo mkdir -p "$local_mnt"
  if mountpoint -q "$local_mnt"; then
    echo "  ok: $local_mnt"
    return 0
  fi
  sudo mount -t nfs4 "${T440}:${remote}" "$local_mnt" -o "$mopts" \
    && echo "  mounted: $local_mnt" \
    || { echo "  FAILED: ${T440}:${remote} -> $local_mnt"; return 1; }
}

fstab_add() {
  local remote="$1" local_mnt="$2" mopts="${3:-$OPTS}"
  grep -qF "$local_mnt" /etc/fstab 2>/dev/null && return 0
  echo "${T440}:${remote} $local_mnt nfs4 ${mopts} 0 0" | sudo tee -a /etc/fstab >/dev/null
}

echo "=== cesarops2 (${NODE_IP}) ← T440 NFS (${T440}) ==="

echo "[1/6] Full data plane mounts"
mount_nfs /data                              /mnt/t440/data
mount_nfs /codebase                          /mnt/t440/codebase
mount_nfs /codebase/repos/wreckhunter2000-1  /mnt/t440/repo
mount_nfs /mnt/raid0                         /mnt/t440/models              "$OPTS"
mount_nfs /home/cesarops                     /mnt/t440/home

echo "[2/6] Legacy paths (compat with existing scripts)"
sudo mkdir -p /data/codebase/repos /data/codebase /mnt/data-external/projects
if ! mountpoint -q /data/codebase/repos/wreckhunter2000-1 2>/dev/null; then
  sudo mount --bind /mnt/t440/repo /data/codebase/repos/wreckhunter2000-1 2>/dev/null || \
    mount_nfs /codebase/repos/wreckhunter2000-1 /data/codebase/repos/wreckhunter2000-1
fi
if ! mountpoint -q /mnt/data-external/projects 2>/dev/null; then
  sudo mkdir -p /mnt/data-external/projects
  sudo mount --bind /mnt/t440/codebase/projects /mnt/data-external/projects
fi
if ! mountpoint -q /mnt/data-external/cesarops 2>/dev/null; then
  sudo mount --bind /mnt/t440/data/cesarops /mnt/data-external/cesarops 2>/dev/null || true
fi

echo "[3/6] fstab persistence"
fstab_add /data                              /mnt/t440/data
fstab_add /codebase                          /mnt/t440/codebase
fstab_add /codebase/repos/wreckhunter2000-1  /mnt/t440/repo
fstab_add /mnt/raid0                         /mnt/t440/models              "$OPTS"
fstab_add /home/cesarops                     /mnt/t440/home

echo "[4/6] Mag path symlinks"
PIPE="/mnt/t440/codebase/projects/pipelines"
MAG="${PIPE}/magnetic_data"
[[ -d "$MAG" ]] && ln -sfn pipelines/magnetic_data /mnt/t440/codebase/projects/magnetic_data 2>/dev/null || true
REPO=/mnt/t440/repo
[[ -d "$MAG" && ! -e "$REPO/magnetic_data" ]] && ln -sfn "$MAG" "$REPO/magnetic_data" 2>/dev/null || true

echo "[5/6] Environment"
ENV="$HOME/.config/cesarops/mag_paths.env"
mkdir -p "$(dirname "$ENV")"
cat >"$ENV" <<EOF
# cesarops2 @ ${NODE_IP} — T440 NFS (${T440})
export T440_IP="${T440}"
export CESAROPS_REPO="${REPO}"
export CESAROPS_PIPELINES="${PIPE}"
export MAG_DATA_ROOT="${MAG}"
export MAG_GRIDS_DIR="${MAG}/grids"
export MAG_RAW_DIR="${MAG}/raw"
export CESAROPS_DATA="/mnt/t440/data/cesarops"
export PATH="\${CESAROPS_REPO}/pipelines/mag:\${PATH}"
EOF
echo "  $ENV"

echo "[6/6] Verify"
source "$ENV"
for f in "$MAG/grids/usgs_namag_82_0000_41_6000__80_0000_42_6000.tif" \
         "$PIPE/mag/forge_cli.py" "$REPO/cesarops-forge-v2/Cargo.toml"; do
  [[ -e "$f" ]] && echo "  OK $(basename "$f")" || echo "  MISSING $f"
done
du -sh "$MAG" 2>/dev/null || true
mount | grep "${T440}:" | sed 's/^/  /'

echo ""
echo "Recovery: edit T440 files via /mnt/t440/{data,home,repo}"
echo "Mag test: source $ENV && cd \$CESAROPS_PIPELINES/mag && python3 forge_cli.py list"
