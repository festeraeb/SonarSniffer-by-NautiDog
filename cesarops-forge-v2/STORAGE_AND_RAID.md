# Storage layout (T440) — May 2026

## RAID status

**No RAID array is active.** `/proc/mdstat` shows no assembled arrays. The two extra drives are **not** configured as RAID10.

## Current layout

| Device | Size | FS | Mount |
|--------|------|-----|-------|
| `sda2` | ~931G | ext4 | `/mnt/data-external` |
| `sdb1` | ~465G | xfs | `/mnt/data-external` and `/codebase` |
| `sdb2` | ~1.7T | xfs | `/data` |
| LVM on `sdc3` | 100G | ext4 | `/` (root — **often full**) |

`/codebase` is on `sdb1` (147G free). Build artifacts should use `CARGO_TARGET_DIR=/data/cargo-target` when `/` is full.

## If you want RAID10 on the new drives

1. Identify the two new block devices (`lsblk`, `fdisk -l`).
2. Stop anything using them; back up data.
3. Example (adjust `/dev/sdX` and `/dev/sdY`):

```bash
sudo mdadm --create /dev/md0 --level=10 --raid-devices=2 /dev/sdX /dev/sdY
sudo mkfs.xfs /dev/md0
sudo mkdir -p /mnt/raid10
sudo mount /dev/md0 /mnt/raid10
```

4. Add an `mdadm` entry to `/etc/mdadm/mdadm.conf` and fstab for persistence.

Until that is done, Forge and models should keep using `/codebase` and `/data` as today.
