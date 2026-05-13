#!/usr/bin/env bash
# Clone HDD (/dev/sdb) to SSD (/dev/sda) on cesarops3
# Run as root or with sudo on the target machine.
#
# What this does:
#   1. Copies the GPT partition table from sdb to sda
#   2. Clones each partition (EFI, boot, LVM) using dd
#   3. Expands the LVM partition to fill the larger SSD
#   4. Updates the bootloader (grub) to point at the SSD
#   5. Logs progress to /tmp/clone_progress.log
#
# After completion, change boot order in BIOS to sda, then reformat sdb as storage.

set -e
LOG="/tmp/clone_progress.log"
SRC="/dev/sdb"
DST="/dev/sda"

log() { echo "[$(date '+%H:%M:%S')] $*" | tee -a "$LOG"; }

log "=== cesarops3 HDD→SSD Clone ==="
log "Source: $SRC (HDD 149GB, current boot)"
log "Dest:   $DST (Crucial MX300 SSD 525GB)"
log ""

# Step 1: Wipe and copy partition table
log "Step 1/5: Copying GPT partition table..."
sfdisk --dump "$SRC" | sfdisk "$DST" 2>&1 | tee -a "$LOG"
partprobe "$DST"
sleep 2
log "  Partition table copied."

# Step 2: Clone EFI partition (sdb1 → sda1, ~1GB vfat)
log "Step 2/5: Cloning EFI partition (sdb1 → sda1)..."
dd if="${SRC}1" of="${DST}1" bs=4M status=progress 2>&1 | tee -a "$LOG"
log "  EFI done."

# Step 3: Clone /boot partition (sdb2 → sda2, ~2GB ext4)
log "Step 3/5: Cloning /boot partition (sdb2 → sda2)..."
dd if="${SRC}2" of="${DST}2" bs=4M status=progress 2>&1 | tee -a "$LOG"
log "  /boot done."

# Step 4: Clone LVM partition (sdb3 → sda3, ~146GB)
log "Step 4/5: Cloning LVM partition (sdb3 → sda3)..."
dd if="${SRC}3" of="${DST}3" bs=4M status=progress 2>&1 | tee -a "$LOG"
log "  LVM partition cloned."

# Step 5: Expand LVM to fill the SSD (sda3 is now 489GB but LVM thinks it's 146GB)
log "Step 5/5: Expanding LVM to fill SSD..."
# Fix the partition UUID so it doesn't conflict with sdb3
NEW_UUID=$(uuidgen)
sfdisk --part-uuid "$DST" 3 "$NEW_UUID" 2>&1 | tee -a "$LOG"

# Activate the cloned VG (it will have the same name — rename to avoid conflict)
vgchange -an ubuntu-vg 2>/dev/null || true
sleep 1

# Import the cloned VG under a temporary name
vgimportclone -n ubuntu-vg-ssd "${DST}3" 2>&1 | tee -a "$LOG" || true
vgchange -ay ubuntu-vg-ssd 2>&1 | tee -a "$LOG" || true

# Resize the PV to fill the new partition
pvresize "${DST}3" 2>&1 | tee -a "$LOG"

# Extend the LV to use all free space
lvextend -l +100%FREE /dev/ubuntu-vg-ssd/ubuntu-lv 2>&1 | tee -a "$LOG" || \
lvextend -l +100%FREE /dev/ubuntu-vg/ubuntu-lv 2>&1 | tee -a "$LOG" || true

# Resize the filesystem
resize2fs /dev/ubuntu-vg-ssd/ubuntu-lv 2>&1 | tee -a "$LOG" || \
resize2fs /dev/ubuntu-vg/ubuntu-lv 2>&1 | tee -a "$LOG" || true

# Update GRUB on the SSD
log "Updating GRUB on SSD..."
mount "${DST}1" /mnt 2>/dev/null || true
mount "${DST}2" /mnt/boot 2>/dev/null || true
grub-install --target=x86_64-efi --efi-directory=/mnt --boot-directory=/mnt/boot \
    --removable 2>&1 | tee -a "$LOG" || true
umount /mnt/boot 2>/dev/null || true
umount /mnt 2>/dev/null || true

log ""
log "=== Clone complete ==="
log "Next steps:"
log "  1. Reboot and change BIOS boot order to boot from sda (SSD)"
log "  2. Verify system boots from SSD"
log "  3. Reformat sdb as storage: mkfs.ext4 -L storage /dev/sdb"
log "  4. Mount sdb at /mnt/storage"
log ""
log "Full log: $LOG"
