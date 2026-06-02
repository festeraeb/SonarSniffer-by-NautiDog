#!/usr/bin/env bash
# Run ON T440 (t440cesarops) as root: sudo bash /path/to/t440-disk-emergency.sh
# Uploaded via SMB to cesarops-data / cesarops-raid. Boot drive should stay small.
set -euo pipefail

CESAROPS2_PUBKEY='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINXtw6y8bOliGvmnUlCcSRUHRlBTG3l5uPMR1oDdKLmT cesarops2-to-t440'
RAID_ROOT="${RAID_ROOT:-}"
DATA_ROOT="${DATA_ROOT:-}"

log() { echo "[t440-disk] $*"; }

find_big_storage() {
  for m in /mnt/cesarops-raid /mnt/storage/cesarops-raid /data/raid /srv/raid; do
    [[ -d "$m" ]] && df -h "$m" 2>/dev/null | tail -1 | grep -qE '[0-9]+G.*[0-9]+%' && RAID_ROOT="$m" && return 0
  done
  # Samba export name often maps here
  for m in /mnt/data-external /data/external; do
    [[ -d "$m" ]] && RAID_ROOT="$m" && return 0
  done
  return 1
}

find_big_storage || log "WARN: could not auto-detect RAID_ROOT; set RAID_ROOT=/your/big/disk"

if [[ -z "$RAID_ROOT" ]]; then
  log "Available mounts:"
  df -hT | grep -E '^/dev|Filesystem'
  exit 1
fi

DATA_ROOT="${DATA_ROOT:-$RAID_ROOT/cesarops-system}"
mkdir -p "$DATA_ROOT"/{cargo-target,npm-cache,docker,journal-archive,logs,tmp}

log "=== BEFORE ==="
df -h /
df -h "$RAID_ROOT"
echo

log "=== Top consumers on ROOT (same filesystem only) ==="
du -xhd1 / 2>/dev/null | sort -hr | head -20
echo
du -xhd1 /var 2>/dev/null | sort -hr | head -12
du -xhd1 /home 2>/dev/null | sort -hr | head -12
du -xhd1 /root 2>/dev/null | sort -hr | head -8
journalctl --disk-usage 2>/dev/null || true
echo

# --- SSH: allow cesarops2 agent in ---
for user_home in /home/cesarops /root; do
  if [[ -d "$user_home" ]]; then
    install -d -m 700 "$user_home/.ssh"
    if ! grep -qF "$CESAROPS2_PUBKEY" "$user_home/.ssh/authorized_keys" 2>/dev/null; then
      echo "$CESAROPS2_PUBKEY" >> "$user_home/.ssh/authorized_keys"
      chmod 600 "$user_home/.ssh/authorized_keys"
      log "Added cesarops2 SSH key to $user_home/.ssh/authorized_keys"
    fi
  fi
done

# --- 1) Journal: cap and vacuum ---
mkdir -p /etc/systemd/journald.conf.d
cat > /etc/systemd/journald.conf.d/99-boot-disk-limit.conf <<'EOF'
[Journal]
SystemMaxUse=500M
RuntimeMaxUse=200M
Storage=persistent
EOF
journalctl --vacuum-size=200M 2>/dev/null || true
systemctl restart systemd-journald 2>/dev/null || true
log "Journal capped + vacuumed"

# --- 2) Docker data-root -> RAID ---
if command -v docker >/dev/null 2>&1; then
  systemctl stop docker 2>/dev/null || true
  mkdir -p "$DATA_ROOT/docker"
  if [[ -d /var/lib/docker && ! -L /var/lib/docker ]]; then
    if [[ "$(du -s /var/lib/docker 2>/dev/null | cut -f1)" -gt 100000 ]]; then
      log "Moving /var/lib/docker -> $DATA_ROOT/docker (rsync)..."
      rsync -aH /var/lib/docker/ "$DATA_ROOT/docker/" || true
      mv /var/lib/docker "/var/lib/docker.bak.$(date +%Y%m%d)"
      ln -sfn "$DATA_ROOT/docker" /var/lib/docker
    fi
  fi
  mkdir -p /etc/docker
  if [[ ! -f /etc/docker/daemon.json ]]; then
    echo "{\"data-root\": \"$DATA_ROOT/docker\"}" > /etc/docker/daemon.json
  fi
  systemctl start docker 2>/dev/null || true
  log "Docker pointed at RAID"
fi

# --- 3) Rust target: never on boot ---
# If project lives on boot (e.g. cesarops-data share path), symlink target to RAID
for proj in /home/cesarops /srv/cesarops /data/cesarops-data; do
  [[ -d "$proj/target" ]] || continue
  if [[ "$(df --output=source "$proj" | tail -1)" == "$(df --output=source / | tail -1)" ]]; then
    log "Moving $proj/target off boot disk..."
    rsync -aH "$proj/target/" "$RAID_ROOT/cargo-target/" 2>/dev/null || true
    mv "$proj/target" "$proj/target.bak.$(date +%Y%m%d)" 2>/dev/null || true
    ln -sfn "$RAID_ROOT/cargo-target" "$proj/target"
  fi
done
# Global env for all users
cat > /etc/profile.d/cesarops-cargo.sh <<EOF
export CARGO_TARGET_DIR="${RAID_ROOT}/cargo-target"
export CARGO_HOME="${RAID_ROOT}/cargo-home"
export NPM_CONFIG_CACHE="${RAID_ROOT}/npm_cache"
EOF
mkdir -p "${RAID_ROOT}/cargo-target" "${RAID_ROOT}/cargo-home" "${RAID_ROOT}/npm_cache"

# --- 4) npm / node tarballs on RAID (already partially there) ---
for d in /home/cesarops/.npm /root/.npm; do
  [[ -d "$d" && ! -L "$d" ]] && { mv "$d" "${d}.bak.$(date +%Y%m%d)"; ln -sfn "${RAID_ROOT}/npm_cache" "$d"; } || true
done

# --- 5) Apt cache ---
if [[ -d /var/cache/apt/archives ]]; then
  du -sh /var/cache/apt/archives
  apt-get clean 2>/dev/null || true
fi

# --- 6) Old logs / tmp ---
find /var/log -type f -name '*.gz' -mtime +14 -delete 2>/dev/null || true
find /var/log -type f -name '*.1' -mtime +7 -delete 2>/dev/null || true
find /tmp -mindepth 1 -mtime +3 -delete 2>/dev/null || true

# --- 7) Snap (often huge on Ubuntu) ---
if command -v snap >/dev/null 2>&1; then
  du -sh /var/lib/snapd 2>/dev/null || true
  LANG=C snap list --all 2>/dev/null | awk '/disabled/{print $1, $2}' | while read -r pkg rev; do
    snap remove "$pkg" --revision="$rev" 2>/dev/null || true
  done
fi

# --- 8) Report models on boot (should be on /codebase or external only) ---
log "=== GGUF on boot filesystem (should be empty) ==="
find /home /root /srv /opt -name '*.gguf' -xdev 2>/dev/null | head -20 || true

log "=== AFTER ==="
df -h /
df -h "$RAID_ROOT"
echo
log "Done. Re-test SSH from cesarops2: ssh -i ~/.ssh/id_ed25519_t440 cesarops@10.0.0.61"
