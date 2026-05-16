#!/bin/bash
# Dispatch to TinyLlama on P1000 (10.0.0.129:5571).
# Task #4: Command buffer replay design pre-screen.
# TinyLlama is too small to write the design itself. Instead we use it to
# DRAFT a checklist of considerations the orchestrator can polish.
# This is the "tiny canary" use of the validator endpoint.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

cat > "$OUT_DIR/r6_picasso_cmd_replay.prompt" << 'PROMPT_EOF'
List the top 5 implementation risks of using wgpu pre-recorded command
buffers (CommandBuffer reuse via record-once-replay-many) for a
transformer per-token decode loop.

Output: numbered list, one risk per line. No prose. No header. Just the list.
PROMPT_EOF

PAYLOAD=$(python3 -c "import json; p=open('$OUT_DIR/r6_picasso_cmd_replay.prompt').read(); print(json.dumps({'prompt': p, 'max_length': 400, 'temperature': 0.3, 'top_p': 0.9}))")

curl -s -m 60 -X POST http://10.0.0.129:5571/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  > "$OUT_DIR/r6_picasso_cmd_replay.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r6_picasso_cmd_replay.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r6_picasso_cmd_replay.txt" 2>"$OUT_DIR/r6_picasso_cmd_replay.err"
echo "[Picasso (P1000) r6] cmd_replay risks: $(wc -l < "$OUT_DIR/r6_picasso_cmd_replay.txt") lines"
