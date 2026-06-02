#!/usr/bin/env bash
set -euo pipefail

# Summarize model scorecard signal for corrector-oriented task types.
# Helps drive per-model tuning decisions without manual JSON digging.

SCORECARD="${SCORECARD:-$HOME/.cache/cesarops/model_scorecard.json}"

if [[ ! -f "$SCORECARD" ]]; then
  echo "scorecard not found: $SCORECARD"
  exit 1
fi

python3 - "$SCORECARD" <<'PY'
import json,sys
p=sys.argv[1]
j=json.load(open(p))
cells=j.get('cells', {})

interesting=('json_repair','loop_judgment','translation','research')
rows=[]
for key,cell in cells.items():
    if '::' not in key:
        continue
    model,task=key.split('::',1)
    if task not in interesting:
        continue
    attempts=cell.get('attempts',0)
    final_pass=cell.get('final_pass',0)
    corrector_eng=cell.get('corrector_engaged',0)
    corrector_help=cell.get('corrector_helped',0)
    uplift=(corrector_help/corrector_eng) if corrector_eng else 0.0
    rate=(final_pass/attempts) if attempts else 0.0
    rows.append((model,task,attempts,rate,uplift,cell.get('recent_outcomes',[])))

rows.sort(key=lambda r:(r[1], -(r[2]), -(r[4]), -(r[3]), r[0]))

print('model\ttask\tattempts\tfinal_pass_rate\tcorrector_uplift\trecent')
for model,task,attempts,rate,uplift,recent in rows:
    print(f"{model}\t{task}\t{attempts}\t{rate:.2f}\t{uplift:.2f}\t{''.join('1' if x else '0' for x in recent)}")

print('\nSuggested tuning hints:')
for model,task,attempts,rate,uplift,recent in rows:
    if attempts < 3:
        continue
    if uplift >= 0.70 and rate < 0.65:
        print(f"- {model} ({task}): keep strict corrector, add tighter loop caps")
    elif uplift < 0.30 and rate >= 0.65:
        print(f"- {model} ({task}): relax corrector coercion; model self-corrects")
    elif uplift < 0.30 and rate < 0.50:
        print(f"- {model} ({task}): route away for this task or increase vector injection")
PY
