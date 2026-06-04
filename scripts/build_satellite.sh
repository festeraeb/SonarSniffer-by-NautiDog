#!/usr/bin/env bash
# Build cesarops-satellite GDAL-free, with the RIGHT CPU target per destination.
#
# target-cpu=native compiles to the BUILD host's full instruction set. That's
# ideal when you build ON the box you run on, but a binary built with native on
# this T440 (Xeon Silver 4110, AVX-512) will SIGILL on the Ivy Bridge HP
# (AVX only, no AVX2). So choose the target by where the binary will RUN.
#
# Usage:
#   build_satellite.sh native     # build for THIS machine (fastest; run here only)
#   build_satellite.sh ivybridge  # fleet-safe floor: AVX, runs on the HP + newer
#   build_satellite.sh haswell    # AVX2/FMA3: Haswell+ (no Ivy Bridge)
#   build_satellite.sh portable   # x86-64-v2 baseline (SSE4.2) — safest, slowest
#
# Default (no arg) = ivybridge, the fleet floor, since the oldest host is Ivy Bridge.
#
# GDAL-free is the default build (no --features gdal). The optional gdal feature
# is only for capable hosts that already have a matching libgdal; do NOT use it
# for fleet-distributed binaries.
set -euo pipefail

TARGET="${1:-ivybridge}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/data/cargo-target}"

case "$TARGET" in
  native)     CPU="native" ;;
  ivybridge)  CPU="ivybridge" ;;   # AVX, no AVX2 — the HP / fleet floor
  haswell)    CPU="haswell" ;;     # AVX2 + FMA3
  portable)   CPU="x86-64-v2" ;;   # SSE4.2 baseline, runs almost anywhere
  *) echo "unknown target '$TARGET' (native|ivybridge|haswell|portable)"; exit 2 ;;
esac

echo "Building cesarops-satellite GDAL-free for target-cpu=$CPU"
echo "  (build host: $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs))"
if [ "$CPU" = "native" ]; then
  echo "  WARNING: native bakes in THIS host's ISA (AVX-512 on the T440)."
  echo "           Run the resulting binary ONLY on this machine."
fi

RUSTFLAGS="-C target-cpu=$CPU" cargo build --release -p cesarops-satellite "${@:2}"
echo "Done. Binary: $CARGO_TARGET_DIR/release/sat-run (target-cpu=$CPU, GDAL-free)"
