#!/usr/bin/env bash
# Scan priority BAG tiles with cesarops-bag-scan; emit JSON next to mission output.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
BAG_ROOT="${BAG_ROOT:-/data/cesarops/bathymetry/straits_surveys}"
OUT="${OUT:-/data/cesarops/satellite_data/detection_runs/straits_local_2024/bag_local}"
BIN="${BAG_SCAN_BIN:-/data/cargo-target/release/bag-scan}"

mkdir -p "$OUT"
if [[ ! -x "$BIN" ]]; then
  echo "building bag-scan..."
  (cd "$REPO" && cargo build --release -p cesarops-bag-scan)
  BIN="/data/cargo-target/release/bag-scan"
fi

for survey in H13255 H13257; do
  dir="$BAG_ROOT/$survey/BAG"
  [[ -d "$dir" ]] || { echo "skip $survey (no $dir)"; continue; }
  for bag in "$dir"/*.bag; do
    [[ -f "$bag" ]] || continue
    [[ "$bag" == *Ellipsoid* ]] && continue
    base=$(basename "$bag" .bag)
    echo "scan $bag ..."
    "$BIN" "$bag" --compact > "$OUT/${survey}_${base}.json" || echo "FAIL $bag"
  done
done
echo "wrote $OUT/*.json"
