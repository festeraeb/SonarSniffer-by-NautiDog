#!/bin/bash
# Setup Samba share on cesarops3 for scratch space
SUDO_PASS="cesarops"
run_sudo() { echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null; }

# Add fstab entry
if ! grep -q "ubuntu--vg--ssd" /etc/fstab; then
    echo "/dev/mapper/ubuntu--vg--ssd-ubuntu--lv /mnt/scratch ext4 defaults,nofail 0 2" | run_sudo tee -a /etc/fstab > /dev/null
    echo "Added to fstab"
fi

# Configure Samba
run_sudo tee /etc/samba/smb.conf > /dev/null << 'EOF'
[global]
   workgroup = CESAROPS
   server string = CESAROPS3 Scratch Storage
   security = user
   map to guest = Bad User

[scratch]
   comment = Fast SSD Scratch Space
   path = /mnt/scratch
   browseable = yes
   read only = no
   guest ok = no
   valid users = cesarops
   create mask = 0664
   directory mask = 0775
EOF

echo -e "cesarops\ncesarops" | run_sudo smbpasswd -a cesarops 2>/dev/null
run_sudo systemctl enable smbd nmbd
run_sudo systemctl restart smbd nmbd
run_sudo mkdir -p /mnt/scratch/pipeline
run_sudo chown cesarops:cesarops /mnt/scratch/pipeline
echo "Done. Share: \\\\100.105.77.74\\scratch (53GB fast SSD)"
