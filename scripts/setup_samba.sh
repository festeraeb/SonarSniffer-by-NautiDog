#!/bin/bash
# Set up Samba shares on T440 for cluster-wide access
set -euo pipefail

REPO="/home/cesarops/wreckhunter2000-1"
source "$REPO/scripts/credentials.sh"

CODEBASE_MOUNT="${CODEBASE_MOUNT:-/codebase}"
DATA_MOUNT="${DATA_MOUNT:-/data}"
MODELS_MOUNT="${MODELS_MOUNT:-/mnt/raid0}"

echo "=== Setting up Samba shares on T440 ==="

# Write share config to temp
cat > /tmp/smb_cesarops.conf << EOF

[cesarops-codebase]
   comment = CESAROPS RAID10 codebase volume
   path = ${CODEBASE_MOUNT}
   browseable = yes
   read only = no
   guest ok = no
   valid users = cesarops
   force user = cesarops
   create mask = 0664
   directory mask = 0775

[cesarops-models]
   comment = CESAROPS RAID0 models + downloads volume
   path = ${MODELS_MOUNT}
   browseable = yes
   read only = no
   guest ok = no
   valid users = cesarops
   force user = cesarops
   create mask = 0664
   directory mask = 0775

[cesarops-data]
   comment = CESAROPS RAID10 data partition
   path = ${DATA_MOUNT}
   browseable = yes
   read only = no
   guest ok = no
   valid users = cesarops
   force user = cesarops
   create mask = 0664
   directory mask = 0775
EOF

# Append to smb.conf if not already there
if ! grep -q "cesarops-codebase" /etc/samba/smb.conf 2>/dev/null || \
   ! grep -q "cesarops-models" /etc/samba/smb.conf 2>/dev/null || \
   ! grep -q "cesarops-data" /etc/samba/smb.conf 2>/dev/null; then
    echo "$SUDO_PASS" | sudo -S cp /etc/samba/smb.conf /etc/samba/smb.conf.bak
    echo "$SUDO_PASS" | sudo -S sh -c 'cat /tmp/smb_cesarops.conf >> /etc/samba/smb.conf'
    echo "  ✓ Shares added to smb.conf"
else
    echo "  ✓ Shares already in smb.conf"
fi

# Set Samba password for cesarops user
echo "$SUDO_PASS" | sudo -S sh -c "printf 'cesarops\ncesarops\n' | smbpasswd -a cesarops -s"
echo "  ✓ Samba password set"

# Restart services
echo "$SUDO_PASS" | sudo -S systemctl restart smbd nmbd
echo "  ✓ smbd + nmbd restarted"

# Verify
echo ""
echo "Shares available:"
echo "$SUDO_PASS" | sudo -S smbclient -L localhost -U cesarops%cesarops -N 2>/dev/null | grep cesarops || echo "  (check manually)"
echo ""
echo "=== Done ==="
echo ""
echo "Map on Windows:"
echo "  net use X: \\\\100.72.182.77\\cesarops-codebase /user:cesarops cesarops /persistent:yes"
echo "  net use Z: \\\\100.72.182.77\\cesarops-data /user:cesarops cesarops /persistent:yes"
echo "  net use Y: \\\\100.72.182.77\\cesarops-models /user:cesarops cesarops /persistent:yes"
