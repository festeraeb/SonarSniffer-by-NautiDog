#!/usr/bin/env bash
# Straits glint accel pass: Coral TPU infer (c2) + Movidius jitter-rs (T440).
# Complements Rust glint_persistence_map from sat-run.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${OUT:-$REPO/var/role_bench/straits_glint_accel}"
TPU="${TPU:-http://10.0.0.201:8092}"
JITTER="${JITTER:-http://10.0.0.61:8180}"
SCENE_ROOT="${SCENE_ROOT:-/mnt/raid0/wreckhunter2000-1-data/data/straits_optical_clear/sentinel2_aws}"
LIMIT="${LIMIT:-0}"

echo "Glint accel: TPU=$TPU JITTER=$JITTER scenes=$SCENE_ROOT -> $OUT"
python3 "$REPO/scripts/role_bench/run_accel_scan_files.py" \
  --out "$OUT" \
  --geotiff-mode scene \
  --prefer-band green \
  --tpu "$TPU" \
  --jitter "$JITTER" \
  --depth-m 34 \
  ${LIMIT:+--limit-scenes "$LIMIT"} \
  "$SCENE_ROOT"

echo "Done. See $OUT/accel_scan_packet.json"
