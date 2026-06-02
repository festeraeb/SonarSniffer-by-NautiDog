#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

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

echo "[1/6] Install packages (nomad consul vector rclone)"
if command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update

  if ! command -v nomad >/dev/null 2>&1 || ! command -v consul >/dev/null 2>&1; then
    install_hashicorp_repo
    sudo apt-get update
    sudo apt-get install -y nomad consul
  fi

  sudo apt-get install -y rclone
  install_vector_binary
else
  echo "apt-get not found; install nomad/consul/vector/rclone manually"
fi

echo "[2/6] Ensure config dirs"
sudo mkdir -p /etc/consul.d /etc/nomad.d /var/lib/consul /var/lib/nomad
sudo chown -R consul:consul /var/lib/consul 2>/dev/null || true

echo "[3/6] Install server configs"
sudo cp "$REPO_ROOT/infra/nomad/consul/server.hcl" /etc/consul.d/consul.hcl
sudo cp "$REPO_ROOT/infra/nomad/nomad/server.hcl" /etc/nomad.d/nomad.hcl

echo "[4/6] Enable services"
sudo systemctl enable --now consul
sudo systemctl enable --now nomad

echo "[5/6] Validate control plane"
consul members || true
nomad server members || true
nomad node status || true

echo "[6/6] Deploy baseline jobs"
nomad job run -detach "$REPO_ROOT/infra/nomad/jobs/forge-api.nomad.hcl"
nomad job run -detach "$REPO_ROOT/infra/nomad/jobs/vector-agent.nomad.hcl"

echo "Bootstrap finished"
