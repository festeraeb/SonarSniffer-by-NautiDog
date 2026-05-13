#!/bin/bash
# Setup TPU passthrough VM on T440
# Binds Coral Edge TPU to vfio-pci, creates a lightweight VM with kernel 6.8
set -euo pipefail
SUDO_PASS="cesarops"
run_sudo() { echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null; }

TPU_PCI="0000:01:00.0"
TPU_VENDOR="1ac1"
TPU_DEVICE="089a"
VM_NAME="tpu-worker"
VM_DISK="/mnt/data-external/cesarops/vm/tpu-worker.qcow2"
VM_RAM=4096  # 4GB
VM_CPUS=2

echo "=== TPU VM Passthrough Setup ==="

# 1. Enable IOMMU in kernel cmdline if not already
echo "[1/5] Checking IOMMU..."
if ! grep -q "intel_iommu=on" /proc/cmdline; then
    echo "  Adding intel_iommu=on to GRUB..."
    run_sudo sed -i 's/GRUB_CMDLINE_LINUX_DEFAULT="\(.*\)"/GRUB_CMDLINE_LINUX_DEFAULT="\1 intel_iommu=on iommu=pt"/' /etc/default/grub
    run_sudo update-grub
    echo "  ⚠ REBOOT REQUIRED for IOMMU to activate"
    echo "  After reboot, re-run this script"
    NEEDS_REBOOT=true
else
    echo "  ✓ IOMMU already enabled"
    NEEDS_REBOOT=false
fi

# 2. Configure vfio-pci to grab the TPU
echo "[2/5] Configuring VFIO for TPU..."
run_sudo tee /etc/modprobe.d/vfio-tpu.conf > /dev/null << EOF
options vfio-pci ids=${TPU_VENDOR}:${TPU_DEVICE}
softdep apex pre: vfio-pci
EOF

# Ensure vfio modules load early
if ! grep -q "vfio-pci" /etc/modules; then
    echo "vfio" | run_sudo tee -a /etc/modules > /dev/null
    echo "vfio_iommu_type1" | run_sudo tee -a /etc/modules > /dev/null
    echo "vfio_pci" | run_sudo tee -a /etc/modules > /dev/null
fi
echo "  ✓ VFIO configured for ${TPU_VENDOR}:${TPU_DEVICE}"

# 3. Create VM disk
echo "[3/5] Creating VM disk..."
run_sudo mkdir -p "$(dirname $VM_DISK)"
if [ ! -f "$VM_DISK" ]; then
    run_sudo qemu-img create -f qcow2 "$VM_DISK" 20G
    echo "  ✓ Created 20GB qcow2 disk"
else
    echo "  ✓ Disk already exists"
fi

# 4. Download Ubuntu 22.04 cloud image (has kernel 6.8 available)
echo "[4/5] Checking cloud image..."
CLOUD_IMG="/mnt/data-external/cesarops/vm/ubuntu-22.04-server-cloudimg-amd64.img"
if [ ! -f "$CLOUD_IMG" ]; then
    echo "  Downloading Ubuntu 22.04 cloud image..."
    run_sudo curl -L -o "$CLOUD_IMG" "https://cloud-images.ubuntu.com/jammy/current/jammy-server-cloudimg-amd64.img"
    echo "  ✓ Downloaded"
else
    echo "  ✓ Cloud image exists"
fi

# 5. Create cloud-init config
echo "[5/5] Creating cloud-init..."
SEED_DIR="/mnt/data-external/cesarops/vm/seed"
run_sudo mkdir -p "$SEED_DIR"

run_sudo tee "$SEED_DIR/user-data" > /dev/null << 'CLOUDINIT'
#cloud-config
hostname: tpu-worker
users:
  - name: cesarops
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    lock_passwd: false
    plain_text_passwd: cesarops
    ssh_authorized_keys: []
packages:
  - python3-pip
  - curl
  - git
runcmd:
  - echo "deb https://packages.cloud.google.com/apt coral-edgetpu-stable main" > /etc/apt/sources.list.d/coral.list
  - curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg | apt-key add -
  - apt-get update
  - apt-get install -y gasket-dkms libedgetpu1-std python3-pycoral
  - pip3 install flask
  - echo "TPU VM ready" > /tmp/tpu-ready
CLOUDINIT

run_sudo tee "$SEED_DIR/meta-data" > /dev/null << EOF
instance-id: tpu-worker-001
local-hostname: tpu-worker
EOF

# Create seed ISO
run_sudo genisoimage -output /mnt/data-external/cesarops/vm/seed.iso -volid cidata -joliet -rock "$SEED_DIR/user-data" "$SEED_DIR/meta-data" 2>/dev/null || \
run_sudo cloud-localds /mnt/data-external/cesarops/vm/seed.iso "$SEED_DIR/user-data" "$SEED_DIR/meta-data" 2>/dev/null || \
echo "  ⚠ Could not create seed ISO (install genisoimage or cloud-image-utils)"

echo ""
echo "=== Setup Complete ==="
echo ""
if [ "${NEEDS_REBOOT:-false}" = "true" ]; then
    echo "⚠ REBOOT REQUIRED: intel_iommu=on was added to GRUB"
    echo "  After reboot, the TPU will be claimed by vfio-pci"
    echo "  Then start the VM with:"
else
    echo "  Start the VM with:"
fi
echo ""
echo "  sudo virt-install \\"
echo "    --name $VM_NAME \\"
echo "    --ram $VM_RAM --vcpus $VM_CPUS \\"
echo "    --disk path=$VM_DISK,format=qcow2 \\"
echo "    --disk path=/mnt/data-external/cesarops/vm/seed.iso,device=cdrom \\"
echo "    --import --os-variant ubuntu22.04 \\"
echo "    --network default \\"
echo "    --hostdev $TPU_PCI \\"
echo "    --graphics none --console pty,target_type=serial \\"
echo "    --noautoconsole"
echo ""
echo "  Then inside the VM:"
echo "    ls /dev/apex_0  (should exist)"
echo "    python3 -c 'import pycoral; print(\"TPU OK\")'"
