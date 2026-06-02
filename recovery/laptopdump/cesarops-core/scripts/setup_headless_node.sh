#!/usr/bin/env bash
# setup_headless_node.sh
# Bootstrap script for headless i7 Optiplex 7010 MT nodes
# Ubuntu Server 22.04 LTS — run as root after base install
#
# Usage:
#   sudo bash setup_headless_node.sh [NODE_NAME] [NFS_SERVER_IP]
#   Example: sudo bash setup_headless_node.sh node1 192.168.10.50
#
# Covers:
#   - Hostname + static IP
#   - Wake-on-LAN (persistent via systemd)
#   - CUDA 12 drivers (Quadro M4000 / Tesla K40 ready)
#   - Python 3.11 + pip + virtualenv
#   - NFS client (for Fortinet 81F USB drives)
#   - SSH hardening
#   - cesarops Python deps

set -euo pipefail

NODE_NAME="${1:-cesarops-node1}"
NFS_SERVER_IP="${2:-}"
CESAROPS_USER="cesarops"
CESAROPS_HOME="/opt/cesarops"
LOG="/var/log/setup_headless_node.log"

log() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG"; }

if [[ "$EUID" -ne 0 ]]; then
    echo "Run as root: sudo bash $0"
    exit 1
fi

log "=== Starting headless node setup: $NODE_NAME ==="

# ── 1. Hostname ────────────────────────────────────────────────────────────────
log "Setting hostname to $NODE_NAME"
hostnamectl set-hostname "$NODE_NAME"
sed -i "/127.0.1.1/d" /etc/hosts
echo "127.0.1.1   $NODE_NAME" >> /etc/hosts

# ── 2. System update ──────────────────────────────────────────────────────────
log "Updating packages"
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get upgrade -y -qq
apt-get install -y \
    curl wget git build-essential \
    python3.11 python3.11-venv python3.11-dev python3-pip \
    ethtool net-tools nmap \
    nfs-common cifs-utils \
    htop iotop nvtop \
    tmux screen \
    gdal-bin python3-gdal \
    libgdal-dev libproj-dev \
    poppler-utils \
    sqlite3 \
    usbutils pciutils \
    unzip zip \
    2>&1 | tail -5

# ── 3. Wake-on-LAN ───────────────────────────────────────────────────────────
log "Configuring Wake-on-LAN"

# Detect primary ethernet interface (not loopback, not wireless)
ETH_IFACE=$(ip link show | awk -F': ' '/^[0-9]+: e/{print $2; exit}')
log "  Ethernet interface: $ETH_IFACE"

# Enable WoL on current boot
ethtool -s "$ETH_IFACE" wol g

# Persist via systemd service
cat > /etc/systemd/system/wol.service << EOF
[Unit]
Description=Enable Wake-on-LAN for $ETH_IFACE
After=network.target

[Service]
Type=oneshot
ExecStart=/sbin/ethtool -s $ETH_IFACE wol g
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable wol.service
log "  WoL enabled on $ETH_IFACE (persists via wol.service)"

# Print MAC for reference (needed to send magic packet from controller)
MAC=$(ip link show "$ETH_IFACE" | awk '/link\/ether/{print $2}')
log "  MAC address: $MAC  <-- record this for wake-on-lan magic packets"

# ── 4. CUDA 12 drivers (Quadro M4000 / Tesla K40) ────────────────────────────
log "Installing NVIDIA CUDA 12 drivers"

# Add NVIDIA CUDA repo for Ubuntu 22.04
wget -q https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2204/x86_64/cuda-keyring_1.1-1_all.deb -O /tmp/cuda-keyring.deb
dpkg -i /tmp/cuda-keyring.deb
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y cuda-drivers 2>&1 | tail -5

# nvidia-smi will fail if no GPU present — that's fine, drivers are staged
log "  CUDA drivers installed. Run 'nvidia-smi' after attaching GPU."

# ── 5. Python virtualenv for cesarops ────────────────────────────────────────
log "Creating cesarops Python environment"

useradd -m -s /bin/bash "$CESAROPS_USER" 2>/dev/null || log "  User $CESAROPS_USER already exists"
mkdir -p "$CESAROPS_HOME"
chown "$CESAROPS_USER:$CESAROPS_USER" "$CESAROPS_HOME"

su - "$CESAROPS_USER" -c "python3.11 -m venv $CESAROPS_HOME/.venv"
su - "$CESAROPS_USER" -c "$CESAROPS_HOME/.venv/bin/pip install --upgrade pip wheel setuptools"
su - "$CESAROPS_USER" -c "$CESAROPS_HOME/.venv/bin/pip install \
    numpy scipy pandas \
    rasterio gdal \
    lightgbm scikit-learn \
    requests beautifulsoup4 lxml \
    PyMuPDF pypdf2 \
    sqlite-utils \
    tqdm paramiko \
    2>&1 | tail -5"

log "  Python venv at $CESAROPS_HOME/.venv"

# ── 6. NFS mount for USB scratch drives (Fortinet 81F drives) ─────────────────
if [[ -n "$NFS_SERVER_IP" ]]; then
    log "Configuring NFS mount from $NFS_SERVER_IP"
    mkdir -p /mnt/scratch

    # Add to fstab (noauto,x-systemd.automount so it doesn't block boot if server is down)
    if ! grep -q "$NFS_SERVER_IP:/srv/scratch" /etc/fstab; then
        echo "$NFS_SERVER_IP:/srv/scratch  /mnt/scratch  nfs  noauto,x-systemd.automount,rw,soft,timeo=30  0  0" >> /etc/fstab
    fi

    systemctl daemon-reload
    log "  NFS mount added to fstab: /mnt/scratch (auto-mount on access)"
else
    log "  No NFS_SERVER_IP provided — skipping NFS config. Run:"
    log "  sudo bash $0 $NODE_NAME <NFS_SERVER_IP>  to add it later."
fi

# ── 7. SSH hardening ──────────────────────────────────────────────────────────
log "Hardening SSH"
SSHD_CONF="/etc/ssh/sshd_config"

# Disable root login, enable key auth
sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin no/' "$SSHD_CONF"
sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' "$SSHD_CONF"
sed -i 's/^#\?PubkeyAuthentication.*/PubkeyAuthentication yes/' "$SSHD_CONF"

# Allow cesarops user
if ! grep -q "AllowUsers" "$SSHD_CONF"; then
    echo "AllowUsers $CESAROPS_USER" >> "$SSHD_CONF"
fi

systemctl reload ssh
log "  SSH: root login disabled, password auth disabled, key auth only"
log "  Add your public key: ssh-copy-id $CESAROPS_USER@$(hostname -I | awk '{print $1}')"

# ── 8. Summary ────────────────────────────────────────────────────────────────
log ""
log "=== Setup complete for $NODE_NAME ==="
log ""
log "  Hostname:     $NODE_NAME"
log "  MAC (WoL):    $MAC"
log "  SSH user:     $CESAROPS_USER"
log "  Python venv:  $CESAROPS_HOME/.venv"
[[ -n "$NFS_SERVER_IP" ]] && log "  Scratch NFS:  /mnt/scratch <- $NFS_SERVER_IP:/srv/scratch"
log ""
log "  NEXT STEPS:"
log "  1. Copy your SSH public key:  ssh-copy-id $CESAROPS_USER@$(hostname -I | awk '{print $1}')"
log "  2. Clone cesarops:            git clone https://github.com/festeraeb/nauticuvs.git $CESAROPS_HOME/cesarops-core"
log "  3. After adding GPU:          reboot && nvidia-smi"
log "  4. Record MAC above for WoL magic packets from controller machine"
