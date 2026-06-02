#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

JOIN_MODE="lan"
if [[ "${3:-}" == "--remote" ]] || [[ "${3:-}" == "remote" ]]; then
  JOIN_MODE="remote"
fi

if [[ $# -lt 2 ]]; then
  echo "Usage: $0 <node_class> <gpu_class> [--remote]"
  echo "  LAN fleet (T440, cesarops2): omit --remote (join T440 at 10.0.0.61)"
  echo "  Laptop when away: --remote (join via Tailscale; set CESAROPS_T440_TS_IP)"
  exit 1
fi

NODE_CLASS="$1"
GPU_CLASS="$2"
T440_LAN="${CESAROPS_T440_LAN:-10.0.0.61}"
T440_TS="${CESAROPS_T440_TS_IP:-100.72.182.77}"

"$REPO_ROOT/infra/nomad/scripts/require_mounted_repo.sh" "$REPO_ROOT"

install_hashicorp_repo() {
  if [[ ! -f /usr/share/keyrings/hashicorp-archive-keyring.gpg ]]; then
    curl -fsSL https://apt.releases.hashicorp.com/gpg | \
      sudo gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg
  fi

  local distro
  distro=$(awk -F= '/^VERSION_CODENAME=/{print $2}' /etc/os-release)
  echo "deb [signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com ${distro} main" | \
    sudo tee /etc/apt/sources.list.d/hashicorp.list >/dev/null
}

install_vector_binary() {
  if command -v vector >/dev/null 2>&1; then
    return
  fi

  curl -fsSL https://sh.vector.dev | sudo bash -s -- -y

  for candidate in /usr/bin/vector /usr/local/bin/vector /root/.vector/bin/vector; do
    if [[ -x "$candidate" ]]; then
      if [[ "$candidate" != /usr/local/bin/vector && ! -x /usr/local/bin/vector ]]; then
        sudo ln -sf "$candidate" /usr/local/bin/vector
      fi
      return
    fi
  done

  echo "ERROR: Vector install completed but binary not found in /usr/bin, /usr/local/bin, or /root/.vector/bin" >&2
  exit 20
}

echo "Install packages"
if command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update

  if ! command -v nomad >/dev/null 2>&1 || ! command -v consul >/dev/null 2>&1; then
    install_hashicorp_repo
    sudo apt-get update
    sudo apt-get install -y nomad consul
  fi

  install_vector_binary
else
  echo "apt-get not found; install nomad/consul/vector manually"
fi

sudo mkdir -p /etc/consul.d /etc/nomad.d /var/lib/consul /var/lib/nomad
sudo chown -R consul:consul /var/lib/consul 2>/dev/null || true
LAN_IP="${CESAROPS_LAN_IP:-$(ip -4 route get 10.0.0.61 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src") print $(i+1)}' | head -1)}"
if [[ -z "$LAN_IP" ]]; then
  LAN_IP="$(hostname -I | awk '{print $2}')"
fi

sudo cp "$REPO_ROOT/infra/nomad/consul/client.hcl" /etc/consul.d/consul.hcl
sudo cp "$REPO_ROOT/infra/nomad/nomad/client.hcl" /etc/nomad.d/nomad.hcl

if [[ "$JOIN_MODE" == "remote" ]]; then
  T440_JOIN="$T440_TS"
  CONSUL_ADVERTISE="${CESAROPS_CONSUL_ADVERTISE:-$(ip -4 addr show tailscale0 2>/dev/null | awk '/inet 100\./ {split($2,a,"/"); print a[1]; exit}')}"
  if [[ -z "$CONSUL_ADVERTISE" ]]; then
    echo "ERROR: --remote requires Tailscale on this host (or CESAROPS_CONSUL_ADVERTISE)" >&2
    exit 21
  fi
else
  T440_JOIN="$T440_LAN"
  CONSUL_ADVERTISE="${CESAROPS_CONSUL_ADVERTISE:-${LAN_IP}}"
fi

if [[ -n "$CONSUL_ADVERTISE" ]]; then
  sudo sed -i "s/bind_addr   = \"10.0.0.201\"/bind_addr   = \"${CONSUL_ADVERTISE}\"/" /etc/consul.d/consul.hcl
  sudo sed -i "s/advertise_addr = \"10.0.0.201\"/advertise_addr = \"${CONSUL_ADVERTISE}\"/" /etc/consul.d/consul.hcl
  # Drop-in wins over WSL auto-detect (172.x) when consul.hcl sed did not apply yet.
  sudo tee /etc/consul.d/10-cesarops-advertise.hcl >/dev/null <<EOF
# CESAROPS: pin LAN or Tailscale (never WSL 172.x)
bind_addr      = "${CONSUL_ADVERTISE}"
advertise_addr = "${CONSUL_ADVERTISE}"
EOF
fi
sudo sed -i "s|retry_join = \\[\"10.0.0.61\"\\]|retry_join = [\"${T440_JOIN}\"]|" /etc/consul.d/consul.hcl

sudo sed -i "s/node_class = \"worker\"/node_class = \"${NODE_CLASS}\"/" /etc/nomad.d/nomad.hcl
sudo sed -i "s/\"node.class\" = \"worker\"/\"node.class\" = \"${NODE_CLASS}\"/" /etc/nomad.d/nomad.hcl
sudo sed -i "s/\"gpu.class\"  = \"rtx2060\"/\"gpu.class\"  = \"${GPU_CLASS}\"/" /etc/nomad.d/nomad.hcl

ZONE="lan"
[[ "$JOIN_MODE" == "remote" ]] && ZONE="tailscale"
sudo sed -i "s/\"zone\"       = \"lan\"/\"zone\"       = \"${ZONE}\"/" /etc/nomad.d/nomad.hcl

# Nomad must not advertise WSL/Docker bridges (172.x); pin RPC/Serf to same IP as Consul.
if [[ -n "$CONSUL_ADVERTISE" ]]; then
  if ! grep -q '^advertise {' /etc/nomad.d/nomad.hcl; then
    sudo tee -a /etc/nomad.d/nomad.hcl >/dev/null <<EOF

advertise {
  http = "${CONSUL_ADVERTISE}"
  rpc  = "${CONSUL_ADVERTISE}"
  serf = "${CONSUL_ADVERTISE}"
}
EOF
  else
    sudo sed -i "s|http = \".*\"|http = \"${CONSUL_ADVERTISE}\"|" /etc/nomad.d/nomad.hcl
    sudo sed -i "s|rpc  = \".*\"|rpc  = \"${CONSUL_ADVERTISE}\"|" /etc/nomad.d/nomad.hcl
    sudo sed -i "s|serf = \".*\"|serf = \"${CONSUL_ADVERTISE}\"|" /etc/nomad.d/nomad.hcl
  fi
  echo "Nomad advertise pinned to ${CONSUL_ADVERTISE} (zone=${ZONE})"
fi

sudo systemctl enable --now consul
sudo systemctl enable --now nomad

consul members || true
nomad node status || true

echo "Client bootstrap finished"
