#!/usr/bin/env bash
# Install system + Python deps on cesarops2 (or any Ubuntu host) to build and run:
#   cesarops-satellite (sat-run)
#   cesarops-aeromagnetic-worker
#   cesarops-bag-scan
#   cesarops-forge-v2 (already on systemd)
#
# Usage:
#   bash scripts/install_pipeline_host_deps.sh          # apt + optional python venv
#   bash scripts/install_pipeline_host_deps.sh --build  # also cargo build --release
#   bash scripts/install_pipeline_host_deps.sh --check  # verify only, no install
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DO_BUILD=0
CHECK_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --build) DO_BUILD=1 ;;
    --check) CHECK_ONLY=1 ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0
      ;;
  esac
done

APT_PACKAGES=(
  # BAG crate: gdal-sys needs gdal.pc (runtime gdal-bin alone is not enough)
  libgdal-dev
  gdal-bin
  gdal-data
  # Rust native deps / probes
  pkg-config
  build-essential
  libclang-dev
  # Mag WGPU backend (usually present on c2; harmless if already installed)
  libvulkan1
  vulkan-tools
  mesa-vulkan-drivers
  # Optional: inspect COGs from shell; python3-gdal pairs with system GDAL 3.8
  python3-gdal
  python3-pip
  python3-venv
)

need_sudo() {
  if [[ "$(id -u)" -eq 0 ]]; then
    SUDO=""
  elif command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    echo "error: need root or sudo to install apt packages" >&2
    exit 1
  fi
}

check_one() {
  local name="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "  ok   $name"
    return 0
  fi
  echo "  MISS $name"
  return 1
}

run_checks() {
  local fail=0
  echo "=== Host checks ==="
  check_one "rustc" rustc --version || fail=1
  check_one "cargo" cargo --version || fail=1
  check_one "gdalinfo" gdalinfo --version || fail=1
  check_one "gdal.pc (pkg-config)" pkg-config --modversion gdal || fail=1
  check_one "vulkan" vulkaninfo --summary || fail=1
  check_one "python3" python3 --version || fail=1
  check_one "credentials.sh" test -f "$REPO/scripts/credentials.sh" || fail=1
  check_one "universal_downloader.py" test -f "$REPO/universal_downloader.py" || fail=1
  if [[ -x "$REPO/.venvs/pipelines/bin/python" ]]; then
    check_one "venv pipelines (requests)" "$REPO/.venvs/pipelines/bin/python" -c "import requests" || fail=1
  else
    echo "  skip venv pipelines (run without --check to create)"
  fi
  for bin in sat-run cesarops-aeromagnetic-worker cesarops-bag-scan cesarops-forge-v2; do
    if [[ -x "$REPO/target/release/$bin" ]]; then
      echo "  ok   target/release/$bin"
    else
      echo "  MISS target/release/$bin"
      fail=1
    fi
  done
  return "$fail"
}

if [[ "$CHECK_ONLY" -eq 1 ]]; then
  run_checks
  exit $?
fi

need_sudo
echo "=== Installing apt packages ==="
$SUDO apt-get update -qq
# --no-install-recommends avoids unrelated DKMS pulls (e.g. gasket-dkms) on some hosts.
if ! $SUDO DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${APT_PACKAGES[@]}"; then
  echo "warn: apt reported errors (often gasket-dkms); checking gdal.pc …" >&2
  if ! pkg-config --exists gdal 2>/dev/null; then
    echo "error: libgdal-dev did not install — fix apt/dpkg then re-run" >&2
    exit 1
  fi
fi

echo "=== Python venv: .venvs/pipelines (satellite download + legacy helpers) ==="
VENV="$REPO/.venvs/pipelines"
if [[ ! -d "$VENV" ]]; then
  python3 -m venv "$VENV"
fi
# shellcheck disable=SC1091
source "$VENV/bin/activate"
pip install -q -U pip
pip install -q requests
# Optional: full Python pipeline parity (heavy; GDAL must match system 3.8)
if [[ "${INSTALL_PY_PIPELINE:-0}" == "1" ]]; then
  pip install -q numpy scipy h5py rasterio
fi
deactivate

echo "=== Credentials reminder ==="
if [[ ! -f "$REPO/scripts/credentials.gemini.local.sh" ]] \
  || ! grep -q 'GEMINI_API_KEY=".\+"' "$REPO/scripts/credentials.gemini.local.sh" 2>/dev/null; then
  echo "  → Set GEMINI_API_KEY in scripts/credentials.gemini.local.sh (Spec Thinker)"
fi
echo "  → source $REPO/scripts/credentials.sh before satellite download (Earthdata/USGS)"

if [[ "$DO_BUILD" -eq 1 ]]; then
  echo "=== cargo build --release (workspace + bag-scan) ==="
  export PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-}"
  cd "$REPO"
  cargo build --release -p cesarops-satellite --bin sat-run
  cargo build --release -p cesarops-aeromagnetic-worker
  (cd "$REPO/cesarops-bag-scan" && cargo build --release)
  cargo build --release -p cesarops-forge-v2
  echo "=== Symlink helpers (optional) ==="
  mkdir -p "$HOME/bin"
  for b in sat-run cesarops-aeromagnetic-worker cesarops-bag-scan; do
    ln -sf "$REPO/target/release/$b" "$HOME/bin/$b" 2>/dev/null || \
      ln -sf "$REPO/cesarops-bag-scan/target/release/$b" "$HOME/bin/$b" 2>/dev/null || true
  done
  # bag-scan may land in its own target/ when built standalone
  if [[ -x "$REPO/cesarops-bag-scan/target/release/cesarops-bag-scan" ]]; then
    ln -sf "$REPO/cesarops-bag-scan/target/release/cesarops-bag-scan" "$HOME/bin/cesarops-bag-scan"
  fi
fi

echo ""
run_checks || true
echo ""
echo "Done. Quick test:"
echo "  sat-run --describe | head"
echo "  cesarops-aeromagnetic-worker describe | head"
echo "  cesarops-bag-scan --describe | head"
echo "  source scripts/credentials.sh && sat-run --help"
