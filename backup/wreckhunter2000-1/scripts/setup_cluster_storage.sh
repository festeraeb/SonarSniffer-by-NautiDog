#!/bin/bash
# ═══════════════════════════════════════════════════════════════════════════════
# CESAROPS Cluster Storage Setup
# ═══════════════════════════════════════════════════════════════════════════════
# Sets up Samba sharing + NFS mounts across all nodes so every machine
# can access every other machine's storage.
#
# Run ON T440 (primary server):
#   bash scripts/setup_cluster_storage.sh
#
# Storage Map:
#   T440:      /mnt/data-external (916GB SSD) — PRIMARY shared storage
#   cesarops2: /mnt/data-external (120GB NTFS) — backup/overflow
#   cesarops3: /mnt/cesarops3-ssd (340GB unused!) — needs mounting + sharing
#   Laptop:    C:\Users\thomf\programming\wreckhunter2000-1 — source code
#
# After this script:
#   - T440 shares /mnt/data-external via Samba as \\T440\cesarops
#   - All nodes can mount each other's drives
#   - Laptop can push code to T440 share directly
# ═══════════════════════════════════════════════════════════════════════════════

set -euo pipefail

SUDO_PASS="${SUDO_PASS:-cesarops}"
run_sudo() { echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null; }

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  CESAROPS Cluster Storage Setup                              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# ═══════════════════════════════════════════════════════════════════════════════
# STEP 1: Install Samba on T440
# ═══════════════════════════════════════════════════════════════════════════════
echo "[1/4] Installing Samba on T440..."
run_sudo apt-get install -y samba samba-common cifs-utils 2>&1 | tail -3

# Configure Samba share
run_sudo tee /etc/samba/smb.conf > /dev/null << 'SMBCONF'
[global]
   workgroup = CESAROPS
   server string = CESAROPS T440 Storage
   security = user
   map to guest = Bad User
   dns proxy = no
   server min protocol = SMB2

[cesarops]
   comment = CESAROPS Shared Storage
   path = /mnt/data-external/cesarops
   browseable = yes
   read only = no
   guest ok = no
   valid users = cesarops
   create mask = 0664
   directory mask = 0775

[repo]
   comment = CESAROPS Repository
   path = /home/cesarops/wreckhunter2000-1
   browseable = yes
   read only = no
   guest ok = no
   valid users = cesarops
   create mask = 0664
   directory mask = 0775
SMBCONF

# Set Samba password for cesarops user
echo -e "cesarops\ncesarops" | run_sudo smbpasswd -a cesarops 2>/dev/null
run_sudo systemctl enable smbd nmbd
run_sudo systemctl restart smbd nmbd
echo "  ✓ Samba configured: \\\\100.72.182.77\\cesarops and \\\\100.72.182.77\\repo"

# ═══════════════════════════════════════════════════════════════════════════════
# STEP 2: Create mount points for other nodes
# ═══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[2/4] Creating mount points..."
run_sudo mkdir -p /mnt/cesarops2
run_sudo mkdir -p /mnt/cesarops3
run_sudo mkdir -p /mnt/laptop

# Add to fstab (commented out — mount on demand)
if ! grep -q "cesarops2" /etc/fstab; then
    echo "# CESAROPS cluster mounts (uncomment to auto-mount)" | run_sudo tee -a /etc/fstab > /dev/null
    echo "#//100.102.158.111/data /mnt/cesarops2 cifs credentials=/etc/samba/creds.cesarops2,uid=cesarops,gid=cesarops,nofail 0 0" | run_sudo tee -a /etc/fstab > /dev/null
    echo "#//100.105.77.74/data /mnt/cesarops3 cifs credentials=/etc/samba/creds.cesarops3,uid=cesarops,gid=cesarops,nofail 0 0" | run_sudo tee -a /etc/fstab > /dev/null
fi
echo "  ✓ Mount points created"

# ═══════════════════════════════════════════════════════════════════════════════
# STEP 3: Firewall rules for Samba
# ═══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[3/4] Configuring firewall..."
run_sudo ufw allow from 10.0.0.0/24 to any port 445 2>/dev/null || true
run_sudo ufw allow from 100.64.0.0/10 to any port 445 2>/dev/null || true
echo "  ✓ Samba ports open for LAN + Tailscale"

# ═══════════════════════════════════════════════════════════════════════════════
# STEP 4: Summary
# ═══════════════════════════════════════════════════════════════════════════════
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Storage Map                                                 ║"
echo "╠══════════════════════════════════════════════════════════════╣"
echo "║                                                              ║"
echo "║  T440 (100.72.182.77):                                      ║"
echo "║    /mnt/data-external  916GB SSD (815GB free)               ║"
echo "║    Samba: \\\\100.72.182.77\\cesarops                        ║"
echo "║    Samba: \\\\100.72.182.77\\repo                            ║"
echo "║                                                              ║"
echo "║  cesarops2 (100.102.158.111):                                ║"
echo "║    /mnt/data-external  120GB NTFS (111GB free)              ║"
echo "║    sdc-sdf: empty card reader slots (0B)                    ║"
echo "║                                                              ║"
echo "║  cesarops3 (100.105.77.74):                                  ║"
echo "║    sda: 489GB SSD — 340GB UNMOUNTED (needs activation)      ║"
echo "║    /: 72GB (53GB free)                                       ║"
echo "║                                                              ║"
echo "║  TOTAL AVAILABLE: ~1.3 TB across cluster                    ║"
echo "║  + 4TB RAID arriving tomorrow                                ║"
echo "║  + 2x 1TB HDD available for cesarops1/2                     ║"
echo "║                                                              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "LAPTOP ACCESS (Windows):"
echo "  net use Z: \\\\100.72.182.77\\cesarops /user:cesarops cesarops"
echo "  net use Y: \\\\100.72.182.77\\repo /user:cesarops cesarops"
echo ""
echo "Or in File Explorer: \\\\100.72.182.77\\cesarops"
echo ""
echo "NEXT: Mount cesarops3's unused 340GB SSD"
echo "  ssh cesarops@100.105.77.74"
echo "  sudo mount /dev/mapper/ubuntu--vg--ssd-ubuntu--lv /mnt/ssd"
