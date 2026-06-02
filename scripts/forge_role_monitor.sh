#!/usr/bin/env bash
set -euo pipefail

FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
MODE="specialized"
LABEL="role-monitor"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) MODE="$2"; shift 2 ;;
    --label) LABEL="$2"; shift 2 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

case "$MODE" in
  same-model)
    THINKER_ENDPOINT="${THINKER_ENDPOINT:-http://127.0.0.1:5200}"
    CODER_ENDPOINT="${CODER_ENDPOINT:-http://127.0.0.1:5200}"
    REVIEWER_ENDPOINT="${REVIEWER_ENDPOINT:-http://127.0.0.1:5200}"
    ;;
  specialized)
    THINKER_ENDPOINT="${THINKER_ENDPOINT:-http://127.0.0.1:5200}"
    CODER_ENDPOINT="${CODER_ENDPOINT:-http://127.0.0.1:5001}"
    REVIEWER_ENDPOINT="${REVIEWER_ENDPOINT:-http://127.0.0.1:5002}"
    ;;
  *)
    echo "Invalid mode: $MODE"
    exit 1
    ;;
esac

OUT_DIR="/tmp/forge_role_monitor_${LABEL}_${MODE}_$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$OUT_DIR"

run_probe() {
  local role="$1" endpoint="$2" prompt="$3"
  local req="$OUT_DIR/${role}_request.json"
  local resp="$OUT_DIR/${role}_response.json"
  local met="$OUT_DIR/${role}_metrics.json"

  python3 - "$req" "$endpoint" "$role" "$prompt" <<'PY'
import json,sys
req,endpoint,role,prompt = sys.argv[1:5]
payload = {
  "suite": "custom",
  "baseline_id": "interactive_fast",
  "endpoint": endpoint,
  "tasks": [{"label": f"{role}-probe", "prompt": prompt}],
}
open(req, 'w').write(json.dumps(payload, indent=2))
PY

  timeout 100 curl --connect-timeout 8 --max-time 45 -sS -X POST "$FORGE_URL/cluster/test/dispatch" \
    -H 'Content-Type: application/json' \
    --data-binary @"$req" \
    -o "$resp" \
    -w '%{time_total}\n' > "$OUT_DIR/${role}_latency_s.txt" || true

  python3 - "$role" "$endpoint" "$resp" "$OUT_DIR/${role}_latency_s.txt" "$met" <<'PY'
import json,re,sys,pathlib
role,endpoint,resp_path,lat_path,out_path = sys.argv[1:6]
lat=0.0
try:
  lat=float(pathlib.Path(lat_path).read_text().strip())
except Exception:
  pass
raw=pathlib.Path(resp_path).read_text(errors='replace') if pathlib.Path(resp_path).exists() else ''
err='no_response'
resp=''
json_ok=False
try:
  j=json.loads(raw)
  json_ok=True
  row=(j.get('results') or [{}])[0]
  agent=row.get('agent') or {}
  err=agent.get('error')
  resp=agent.get('response') or ''
except Exception as e:
  err=f'parse_error:{e}'

txt=resp.lower()
has_commands=bool(re.search(r'(^|\n)\s*(\d+\.|[-*])\s*(```bash|`?(sudo|bash|grep|sed|awk|cat|ls|find|realpath|n8n|curl|systemctl|journalctl|cp|mv|rm|git|cargo)\b)', txt))
has_path=bool(re.search(r'/(etc|usr|var|opt|home|tmp|data)/|[\w./-]+\.(sh|json|ya?ml|toml|rs)\b', txt))
has_verify=any(k in txt for k in ['verify','verification','test','smoke','health','curl'])
has_rollback=any(k in txt for k in ['rollback','backup','restore'])
has_placeholder=any(k in txt for k in ['/path/to', 'your-repo', 'your-username', 'example'])
has_meta=any(k in txt for k in ['<think>', "here's a thinking process", 'analyze user input', 'baseline:'])
generic_scaffold=any(k in txt for k in ['requirements:', 'proposed fix plan', '[ ] passed'])

score=0
if json_ok and not err and resp.strip():
  score += 35
if len(resp) >= 220:
  score += 10
elif len(resp) >= 140:
  score += 5
if has_commands:
  score += 20
if has_path:
  score += 15
if has_verify:
  score += 10
if has_rollback:
  score += 10
if lat > 0:
  score += 5 if lat <= 40 else (3 if lat <= 85 else 1)

if has_placeholder:
  score -= 30
if has_meta:
  score -= 30
if generic_scaffold and not has_commands:
  score -= 20
if ('unknown' in txt or 'need more information' in txt) and len(resp) < 220:
  score -= 15

score=max(0,min(100,score))
status='good' if score>=85 else ('ok' if score>=65 else 'weak')

out={
  'role': role,
  'endpoint': endpoint,
  'latency_s': lat,
  'json_ok': json_ok,
  'error': err,
  'response_chars': len(resp),
  'score': score,
  'status': status,
  'flags': {
    'has_commands': has_commands,
    'has_path': has_path,
    'has_verify': has_verify,
    'has_rollback': has_rollback,
    'has_placeholder': has_placeholder,
    'has_meta': has_meta,
    'generic_scaffold': generic_scaffold,
  },
  'preview': resp[:280],
}
pathlib.Path(out_path).write_text(json.dumps(out, indent=2))
print(json.dumps(out))
PY
}

run_probe "thinker" "$THINKER_ENDPOINT" "Give exactly 5 bullets: (1) root-cause hypothesis, (2) discovery command, (3) exact file path to inspect, (4) rollback command, (5) verification command. No <think> block. No placeholders like /path/to or your-repo."
run_probe "coder" "$CODER_ENDPOINT" "Return exactly: 1) 4 shell commands with concrete paths, 2) file edit snippet for the failing script line, 3) rollback command, 4) verification command list. No placeholders, no generic examples, no <think>."
run_probe "reviewer" "$REVIEWER_ENDPOINT" "Audit a fix for n8n bad substitution/path issue. Return a PASS/FAIL checklist with required evidence lines, then one rollback command and one verification command. Reject placeholders and reject responses without concrete commands/paths."

python3 - "$OUT_DIR" <<'PY'
import json, pathlib, sys
out=pathlib.Path(sys.argv[1])
rows=[]
for p in sorted(out.glob('*_metrics.json')):
  rows.append(json.loads(p.read_text()))
avg=sum(r.get('score',0) for r in rows)/len(rows) if rows else 0.0
summary={
  'out_dir': str(out),
  'mode': out.name,
  'roles': rows,
  'avg_score': avg,
}
(out/'summary.json').write_text(json.dumps(summary, indent=2))
print(json.dumps(summary, indent=2))
PY
