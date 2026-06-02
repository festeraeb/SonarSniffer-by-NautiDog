#!/usr/bin/env bash
# setup_nfs_scratch.sh
# Configure one of the i7s as the NFS scratch server
# using the Fortinet 81F internal drives mounted via USB enclosures.
#
# Run on whichever i7 has the drives attached (the "storage node").
# The other i7 (compute node) runs setup_headless_node.sh with this machine's IP.
#
# Usage:
#   sudo bash setup_nfs_scratch.sh [ALLOWED_SUBNET]
#   Example: sudo bash setup_nfs_scratch.sh 192.168.10.0/24

set -euo pipefail

SUBNET="${1:-192.168.10.0/24}"
SCRATCH_DIR="/srv/scratch"
LOG="/var/log/setup_nfs_scratch.log"

log() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG"; }

if [[ "$EUID" -ne 0 ]]; then
    echo "Run as root: sudo bash $0"
    exit 1
fi

log "=== NFS scratch server setup ==="

apt-get install -y nfs-kernel-server nfs-common 2>&1 | tail -3

# ── Detect USB drives ────────────────────────────────────────────────────────
log "Detecting USB storage devices:"
lsblk -o NAME,SIZE,TYPE,TRAN,MOUNTPOINT | grep usb || log "  (no USB drives detected yet — plug in before continuing)"
log ""
log "Available block devices:"
lsblk -o NAME,SIZE,TYPE,FSTYPE,LABEL 2>/dev/null | grep -v loop

# ── Partition and format instructions (manual) ───────────────────────────────
log ""
log "  If drives are unformatted, run:"
log "  sudo mkfs.ext4 -L scratch0 /dev/sdX     # first Fortinet drive"
log "  sudo mkfs.ext4 -L scratch1 /dev/sdY     # second Fortinet drive"
log ""
log "  Then update /etc/fstab manually with the UUIDs:"
log "  blkid /dev/sdX"

# ── Create mount points ───────────────────────────────────────────────────────
mkdir -p /mnt/fortinet0 /mnt/fortinet1
mkdir -p "$SCRATCH_DIR"

# ── Bind-mount scratch dir from both drives (union via same export) ───────────
# In practice, run jobs writing to /mnt/fortinet0 or /mnt/fortinet1 directly
# and symlink job scratch dirs as needed. This exports the union point.
if ! mountpoint -q /mnt/fortinet0; then
    log "  /mnt/fortinet0 not mounted — add fstab entry after formatting"
fi

# Create working scratch structure
mkdir -p "$SCRATCH_DIR/downloads" "$SCRATCH_DIR/tiles" "$SCRATCH_DIR/ml_scratch" "$SCRATCH_DIR/scan_cache"
chown -R nobody:nogroup "$SCRATCH_DIR"
chmod 777 "$SCRATCH_DIR"

# ── NFS exports ──────────────────────────────────────────────────────────────
log "Configuring NFS exports"

EXPORT_LINE="$SCRATCH_DIR  $SUBNET(rw,sync,no_subtree_check,no_root_squash)"

if ! grep -qF "$SCRATCH_DIR" /etc/exports; then
    echo "$EXPORT_LINE" >> /etc/exports
fi

exportfs -ra
systemctl enable nfs-kernel-server
systemctl restart nfs-kernel-server

MY_IP=$(hostname -I | awk '{print $1}')
log ""
log "=== NFS server ready ==="
log "  Export:       $SCRATCH_DIR -> $SUBNET"
log "  Server IP:    $MY_IP"
log ""
log "  On compute nodes, mount with:"
log "  sudo mount $MY_IP:/srv/scratch /mnt/scratch"
log "  OR use setup_headless_node.sh $MY_IP to configure automount"
log ""
log "  Verify from another machine:"
log "  showmount -e $MY_IP"
