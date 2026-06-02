#!/usr/bin/env bash
# End-to-end: cesarops2 LLM endpoints + Forge (T440) routing + Holloway SonarSniffer outputs.
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
FORGE="${FORGE_URL:-http://10.0.0.61:9100}"
RSD="$REPO/sonarsniffer/test_files/515456/Holloway.RSD"
PARSE="$REPO/sonarsniffer/target/release/parse_cli"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$REPO/outputs/e2e_forge_holloway_$STAMP"
REPORT="$OUT/E2E_REPORT.json"
LOG="$OUT/e2e.log"

mkdir -p "$OUT"
exec > >(tee -a "$LOG") 2>&1

log() { echo "[e2e] $*"; }

json() { python3 -c 'import sys,json; print(json.dumps(json.load(sys.stdin), indent=2))' 2>/dev/null || cat; }

log "=== E2E: Forge + Holloway ==="
log "FORGE=$FORGE"
log "OUT=$OUT"

# ── 1) Start cesarops2 LLM stack (thinker :5200, draft :5571) ──
log "Step 1: cesarops2 research lab (thinker + draft)"
bash "$REPO/scripts/cesarops2_research_lab.sh" start
sleep 3

# ── 2) cesarops-node heartbeat to Forge ──
log "Step 2: cesarops-node"
pkill -f 'target/release/cesarops-node.*cesarops-node-cesarops2' 2>/dev/null || true
sleep 1
nohup "$REPO/target/release/cesarops-node" "$REPO/cesarops-node/cesarops-node-cesarops2.toml" \
  >>/tmp/cesarops-node.log 2>&1 &
sleep 4
curl -sf "http://127.0.0.1:5500/health" | json | head -20 || log "WARN: node :5500 health failed"

# ── 3) Verify LLM endpoints (cesarops2 NIC .201) ──
log "Step 3: LLM health"
for url in \
  "http://10.0.0.201:5200/health" \
  "http://127.0.0.1:5200/health" \
  "http://10.0.0.201:5571/health" \
  "http://127.0.0.1:5571/health"; do
  if curl -sf --max-time 5 "$url" >/dev/null; then
    log "  OK $url"
  else
    log "  FAIL $url"
  fi
done

# ── 4) Forge on T440 (NOT 127.0.0.1:9100 on cesarops2) ──
log "Step 4: Forge $FORGE"
curl -sf --max-time 10 "$FORGE/health" | json | head -15 || { log "FAIL: Forge unreachable"; exit 1; }

log "Step 4b: Apply routing preset golden-test-b5"
curl -sf --max-time 30 -X POST "$FORGE/cluster/routing/preset/golden-test-b5" \
  -H 'Content-Type: application/json' \
  -d '{"start_workers":false}' | json | tee "$OUT/forge_preset.json" | head -30

log "Step 4c: validate/ping (LLM roles)"
curl -sf --max-time 180 "$FORGE/validate/ping" | json | tee "$OUT/forge_validate_ping.json" | head -40

log "Step 4d: B5 golden dispatch"
curl -sf --max-time 600 -X POST "$FORGE/cluster/test/dispatch" \
  -H 'Content-Type: application/json' \
  -d '{"suite":"B5","baseline_id":"interactive_fast","endpoint":"http://10.0.0.201:5200"}' \
  | json | tee "$OUT/forge_b5_dispatch.json" | head -50

# ── 5) Holloway SonarSniffer full pipeline ──
log "Step 5: Holloway parse_cli"
"$PARSE" "$RSD" --output-dir "$OUT" --summary 2>&1 | tee "$OUT/holloway_parse.json" | tail -25

RUN_DIR=$(find "$OUT" -maxdepth 1 -type d -name 'Holloway*' | head -1)
log "Step 5b: artifact check $RUN_DIR"
MISS=0
for f in waterfall_ch4.png waterfall_ch5.png mosaic_combined.png mosaic_geographic.png \
  sonar_waterfall_enhanced.mp4 track.kmz sonar.mbtiles; do
  if [[ -f "$RUN_DIR/$f" ]]; then
    ls -lh "$RUN_DIR/$f" | awk '{print "  OK", $9, $5}'
  else
    echo "  MISSING $f"; MISS=$((MISS+1))
  fi
done

if [[ -f "$RUN_DIR/sonar_waterfall_enhanced.mp4" ]]; then
  ffprobe -v error -show_entries format=duration,size \
    -show_entries stream=width,height,nb_frames,codec_name \
    -of default=noprint_wrappers=1 "$RUN_DIR/sonar_waterfall_enhanced.mp4" \
    | tee "$OUT/video_probe.txt"
fi

# ── 6) GPU snapshot ──
log "Step 6: nvidia-smi"
nvidia-smi --query-gpu=index,name,utilization.gpu,memory.used,power.draw --format=csv | tee "$OUT/gpu.csv"

# ── Summary JSON ──
python3 <<PY
import json, pathlib
out = pathlib.Path("$OUT")
report = {
  "status": "ok" if $MISS == 0 else "partial",
  "timestamp_utc": "$STAMP",
  "forge_url": "$FORGE",
  "holloway_output_dir": "$RUN_DIR",
  "artifacts_missing": $MISS,
  "log": "$LOG",
}
for name in ["forge_preset.json", "forge_validate_ping.json", "forge_b5_dispatch.json", "holloway_parse.json"]:
    p = out / name
    if p.exists():
        try:
            report[name.replace(".json","")] = json.loads(p.read_text())
        except Exception as e:
            report[name.replace(".json","")] = {"error": str(e)}
out.joinpath("E2E_REPORT.json").write_text(json.dumps(report, indent=2))
print(json.dumps(report, indent=2)[:2000])
PY

log "DONE → $REPORT"
