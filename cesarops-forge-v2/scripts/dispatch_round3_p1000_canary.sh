#!/bin/bash
# Dispatch a tiny task to the P1000 TinyLlama validator (cesarops2:5571).
# Tiny model = small task: validate that a single Rust expression is syntactically OK.
# This warms the validator path so the orchestrator can use it for cheap checks.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

PROMPT='You are a Rust syntax checker. Reply with ONLY one word: VALID or INVALID.

Code: `let x: u32 = (n + 255) / 256;`'

ESC=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 30 -X POST http://10.0.0.129:5571/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC, \"max_length\": 10, \"temperature\": 0.0, \"top_p\": 1.0}" \
  > "$OUT_DIR/r3_p1000_canary.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r3_p1000_canary.json')); print(d.get('results',[{}])[0].get('text','').strip())" \
  > "$OUT_DIR/r3_p1000_canary.out"
echo "[P1000 canary] -> $(cat "$OUT_DIR/r3_p1000_canary.out")"
