#!/usr/bin/env bash
# Prove satellite + accel tools against Michigan Preserves dive-verified wrecks in AOI.
# BAG (cesarops-bag-scan) is a separate verification tool — not required here.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
SPEC="$REPO/data/missions/straits_local_run.json"
ROOT="${SAT_ROOT:-/data/cesarops/satellite_data}"
SAT_RUN="${SAT_RUN:-/data/cargo-target/release/sat-run}"
OUT="$ROOT/detection_runs/straits_local_2024"

echo "=== 1) sat-run (preserve GT: gt_min_confidence=1.0, dive_verified in bbox) ==="
"$SAT_RUN" --spec "$SPEC" --root "$ROOT" 2>&1 | tee "$OUT/prove_tools_sat_run.log" | tail -40

echo ""
echo "=== 2) glint accel smoke (optional corroboration) ==="
if [[ -d /mnt/raid0/wreckhunter2000-1-data/data/straits_optical_clear/sentinel2_aws ]]; then
  LIMIT="${LIMIT:-3}" TPU="${TPU:-http://127.0.0.1:8092}" \
    bash "$REPO/scripts/role_bench/run_straits_glint_accel.sh" || true
  PERSIST="$OUT/temporal_stack/glint_persistence_map.json"
  ACCEL="$REPO/var/role_bench/straits_glint_accel/accel_scan_packet.json"
  if [[ -f "$PERSIST" && -f "$ACCEL" ]]; then
    python3 "$REPO/scripts/role_bench/fuse_glint_accel.py" \
      --persist "$PERSIST" --accel "$ACCEL" \
      --out "$OUT/prove_tools_glint_fused.json"
  fi
else
  echo "(skip accel: NFS optical path missing)"
fi

echo ""
echo "=== 3) validation report ==="
VR="$OUT/validation_report.json"
if [[ -f "$VR" ]]; then
  python3 - <<'PY' "$VR"
import json, sys
vr = json.load(open(sys.argv[1]))
print(f"GT wrecks: {vr.get('n_gt')}  pass: {vr.get('n_pass')}")
for e in vr.get("entries") or []:
    print(f"  {e.get('status'):12} {e.get('name','')[:40]:40} score={e.get('score',0):.3f} concept={e.get('concept','')}")
PY
else
  echo "no validation_report.json"
fi

echo ""
echo "=== 4) BAG (independent) — run only when you want bathy corroboration ==="
echo "  bash $REPO/scripts/role_bench/run_straits_bag_scan.sh"
