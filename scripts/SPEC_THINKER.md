# Spec Thinker roundtrip

Wired into Forge (`POST /spec/draft`, `/spec/approve`, `/spec/status`, `/spec/clear`):


```
Operator intent → Gemini (structured JSON) → local compiler → optional RTX/P100 worker
                      ↘ local fallback (:5200 / :5203) if API fails
```

## Setup

```bash
# Preferred: edit scripts/credentials.sh (see GEMINI_API_KEY= slot), then:
source /data/codebase/repos/wreckhunter2000-1/scripts/credentials.sh

# Or one-off:
export GEMINI_API_KEY="..."   # https://aistudio.google.com/apikey
export GEMINI_MODEL="gemini-2.5-flash"
```

`spec_thinker_roundtrip.py` auto-sources `scripts/credentials.sh` if `GEMINI_API_KEY` is not already set.

Optional local fallback (no cloud):

```bash
bash scripts/cesarops2_unified_layout.sh start   # RTX :5200 + ZAYA :5203
```

## Run

```bash
cd /data/codebase/repos/wreckhunter2000-1

# Gemini structured output (recommended)
python3 scripts/spec_thinker_roundtrip.py \
  --intent "In cesarops-forge-v2, corrector_hard_repeat_cap=6 should demand progress before terminate at 12"

# Local only (8B on RTX/1070 — bench with scripts/role_bench/run_spec_draft_bench.sh)
python3 scripts/spec_thinker_roundtrip.py --local-only --intent "..."

# Dry-run worker prompts (no P100 unless you point --worker-url)
python3 scripts/spec_thinker_roundtrip.py --intent "..." --dispatch-dry-run

# Recovery sandbox: require git branch spec/*
python3 scripts/spec_thinker_roundtrip.py --intent "..." --git-check
```

Output: `var/spec_thinker/<timestamp>/operator_spec.json`

## Schema

`scripts/spec_thinker_schema.json` — enforced by Gemini `responseJsonSchema`.

Each task:

- `target_file_path`
- `injected_context_files[]`
- `explicit_steps[]`
- `done_condition`

Top-level `needs_user_approval` must be `true`.

## Compare to Gemini’s sample

| Gemini snippet | This repo |
|----------------|-----------|
| Wrong URL `googleapis.com{KEY}` | `generativelanguage.googleapis.com/v1beta/models/...:generateContent?key=` |
| `responseSchema` + Pydantic only | `responseJsonSchema` + committed JSON schema (no pydantic install required) |
| `parse_raw` | `load_spec_json` + dataclasses |
| Ollama `11434` | Fleet llama-server `:5200` / `:5203` / P100 `:5001` via `SPEC_WORKER_URL` |

## Human gate (Forge)

1. `POST /spec/draft` with `{"intent":"..."}` (uses `credentials.gemini.local.sh` when thinker is gemini/auto).
2. Review `var/spec_thinker/latest/operator_spec.json`.
3. `POST /spec/approve` with `{"approved":true}` (or **✓ APPROVE** in the Forge UI).
4. `/send` — approved spec is injected into steering; unapproved specs block `/send` when `[spec_draft] require_approval_before_send = true`.

```bash
curl -s -X POST http://127.0.0.1:9100/spec/draft \
  -H 'Content-Type: application/json' \
  -d '{"intent":"cap forge corrector repeats at 6"}'
curl -s -X POST http://127.0.0.1:9100/spec/approve -H 'Content-Type: application/json' -d '{"approved":true}'
```

## Model bench (local)

```bash
bash scripts/role_bench/run_spec_draft_bench.sh
```

See `scripts/role_bench/SPEC_DRAFT_BENCH.md`.
