#!/usr/bin/env bash
# Full Straits detection path — no Forge. Cursor + Gemini tune from outputs.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
SPEC="$REPO/data/missions/straits_local_run.json"
ROOT="${SAT_ROOT:-/data/cesarops/satellite_data}"
SAT_RUN="${SAT_RUN:-/data/cargo-target/release/sat-run}"
OUT="$ROOT/detection_runs/straits_local_2024"
REPORT="$OUT/detection_path_report.json"
TS="$(date -u +%Y%m%dT%H%M%SZ)"

mkdir -p "$OUT"
log() { echo "[detection-path] $*"; }

log "=== build sat-run (gdal) ==="
if [[ ! -x "$SAT_RUN" ]]; then
  (cd "$REPO" && cargo build --release -p cesarops-satellite --features gdal)
  SAT_RUN="/data/cargo-target/release/sat-run"
fi

log "=== sat-run mission ==="
SAT_LOG="$OUT/detection_path_${TS}.log"
"$SAT_RUN" --spec "$SPEC" --root "$ROOT" 2>&1 | tee "$SAT_LOG" || true

log "=== optional glint accel + fuse ==="
ACCEL_OK=false
FUSED=""
if [[ -d /mnt/raid0/wreckhunter2000-1-data/data/straits_optical_clear/sentinel2_aws ]]; then
  LIMIT="${LIMIT:-0}" TPU="${TPU:-http://127.0.0.1:8092}" \
    bash "$REPO/scripts/role_bench/run_straits_glint_accel.sh" && ACCEL_OK=true || true
  PERSIST="$OUT/temporal_stack/glint_persistence_map.json"
  ACCEL="$REPO/var/role_bench/straits_glint_accel/accel_scan_packet.json"
  if [[ -f "$PERSIST" && -f "$ACCEL" ]]; then
    FUSED="$OUT/detection_path_glint_fused.json"
    python3 "$REPO/scripts/role_bench/fuse_glint_accel.py" \
      --persist "$PERSIST" --accel "$ACCEL" --out "$FUSED" || true
  fi
fi

log "=== summarize for Gemini / collab ==="
python3 - <<'PY' "$REPORT" "$OUT" "$SAT_LOG" "$ACCEL_OK" "$FUSED" "$TS"
import json, sys
from pathlib import Path

report_path, out, sat_log, accel_ok, fused, ts = sys.argv[1:7]
out = Path(out)
summary = {
    "ts": ts,
    "mission_spec": "data/missions/straits_local_run.json",
    "sat_log": sat_log,
    "accel_ran": accel_ok == "true",
    "glint_fused": fused if fused else None,
    "artifacts": {
        "validation_report": str(out / "validation_report.json"),
        "glint_persistence_map": str(out / "temporal_stack/glint_persistence_map.json"),
        "bathy_map": str(out / "bathy_map/bathymetry_stack_report.json"),
        "mission_report": str(out / "mission_report.json"),
    },
}
vr = out / "validation_report.json"
if vr.is_file():
    summary["validation"] = json.loads(vr.read_text())
for name in ("mission_report.json",):
    p = out / name
    if p.is_file():
        try:
            summary["mission"] = json.loads(p.read_text())
        except json.JSONDecodeError:
            pass
if fused:
    fp = Path(fused)
    if fp.is_file():
        summary["glint_fuse"] = json.loads(fp.read_text())
Path(report_path).write_text(json.dumps(summary, indent=2))
print(json.dumps({"wrote": report_path, "n_gt": summary.get("validation", {}).get("n_gt")}, indent=2))
PY

log "wrote $REPORT"
log "Gemini paste: var/forge_collab/outbox/GEMINI_DETECTION_PATH_ROUND6.md"
log "Next: tune from report; P100 offload per docs/SATELLITE_P100_OFFLOAD.md"
