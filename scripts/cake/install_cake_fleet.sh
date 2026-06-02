#!/usr/bin/env bash
# Install evilsocket Cake (cake-cli) for fleet idle audit. Safe during pipeline — no fleet timer enable.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
INSTALL_ROOT="${INSTALL_ROOT:-/opt/cesarops/cake}"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
METHOD="${METHOD:-auto}"   # auto | crates | git
FEATURES="${CAKE_FEATURES:-cuda}"
CAKE_VERSION="${CAKE_VERSION:-}"

log() { echo "[install_cake] $*"; }

setup_cuda_env() {
  for d in /usr/local/cuda-12.6 /usr/local/cuda-12.4 /usr/local/cuda; do
    if [[ -x "$d/bin/nvcc" ]]; then
      export PATH="$d/bin:$PATH"
      export LD_LIBRARY_PATH="$d/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
      export CUDA_HOME="$d"
      log "CUDA toolkit: $d ($(nvcc --version 2>/dev/null | grep release | head -1))"
      return 0
    fi
  done
  log "WARN: no /usr/local/cuda-* nvcc — using system CUDA if any"
}

# candle-kernels bf16 WMMA MoE kernels fail to compile for sm_75 (Turing / RTX 2060).
# Build with a higher cap; runtime still uses the local NVIDIA GPU via cudarc.
pick_cuda_compute_cap() {
  if [[ -n "${CUDA_COMPUTE_CAP:-}" ]]; then
    return 0
  fi
  local cap
  cap=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | head -1 | tr -d ' ' || true)
  if [[ "$cap" == "7.5" ]]; then
    export CUDA_COMPUTE_CAP=86
    log "Turing (cc $cap) — CUDA_COMPUTE_CAP=86 for cargo build (install workaround)"
  fi
}

ensure_rust() {
  if command -v cargo >/dev/null 2>&1; then
    log "cargo: $(cargo --version)"
    return 0
  fi
  log "Installing rustup (non-interactive)…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck source=/dev/null
  source "${CARGO_HOME}/env"
}

install_cake_cli() {
  ensure_rust
  # shellcheck source=/dev/null
  source "${CARGO_HOME}/env" 2>/dev/null || true

  local -a install_args=(--locked --features "$FEATURES")
  [[ -n "$CAKE_VERSION" ]] && install_args+=(--version "$CAKE_VERSION")
  [[ "${CAKE_INSTALL_FORCE:-0}" == "1" ]] && install_args+=(--force)

  if [[ "$METHOD" == "git" ]]; then
    log "cargo install cake-cli from GitHub (main)…"
    cargo install --git https://github.com/evilsocket/cake.git cake-cli "${install_args[@]}"
  elif [[ "$METHOD" == "crates" ]]; then
    log "cargo install cake-cli from crates.io…"
    cargo install cake-cli "${install_args[@]}"
  else
    log "Trying crates.io cake-cli, then GitHub…"
    if ! cargo install cake-cli "${install_args[@]}" 2>/tmp/cake-install-crates.log; then
      log "crates.io failed ($(tail -1 /tmp/cake-install-crates.log)); falling back to git"
      cargo install --git https://github.com/evilsocket/cake.git cake-cli "${install_args[@]}"
    fi
  fi
}

link_system_path() {
  local bin="${CARGO_HOME}/bin/cake"
  [[ -x "$bin" ]] || { log "ERROR: $bin not found after install"; exit 1; }
  sudo mkdir -p "${INSTALL_ROOT}/bin"
  sudo ln -sf "$bin" "${INSTALL_ROOT}/bin/cake"
  log "Linked ${INSTALL_ROOT}/bin/cake -> $bin"
  "$bin" --version 2>/dev/null || "$bin" --help 2>&1 | head -3
}

setup_cluster_key() {
  local keyfile="/etc/cesarops/cake-cluster.key"
  if [[ -f "$keyfile" ]]; then
    log "Cluster key already exists: $keyfile"
    return 0
  fi
  log "Creating cluster key (sudo)…"
  sudo mkdir -p /etc/cesarops
  sudo bash -c "umask 077; openssl rand -hex 24 > '$keyfile'"
  sudo chown root:cesarops "$keyfile" 2>/dev/null || true
  log "Wrote $keyfile (readable by cesarops group if configured)"
}

main() {
  log "Install root: $INSTALL_ROOT"
  setup_cuda_env
  pick_cuda_compute_cap
  if ! command -v nvidia-smi >/dev/null 2>&1; then
    log "WARN: nvidia-smi not found — build may fall back to CPU"
  fi
  install_cake_cli
  link_system_path
  setup_cluster_key

  log "Pull idle audit model (optional, large download):"
  log "  ${INSTALL_ROOT}/bin/cake pull ${CAKE_MODEL_35B:-Qwen/Qwen3.6-35B-A3B-Instruct}"
  log "After pipeline completes:"
  log "  sudo touch /etc/cesarops/fleet-mode.enabled"
  log "  sudo cp ${REPO}/systemd/cesarops-activity-watch.* /etc/systemd/system/"
  log "  sudo systemctl daemon-reload && sudo systemctl enable --now cesarops-activity-watch.timer"
  log "Do NOT enable fleet timer while satellite pipeline missions are running."
}

main "$@"
