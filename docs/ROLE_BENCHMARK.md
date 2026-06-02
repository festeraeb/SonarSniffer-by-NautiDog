# Role benchmark tournament

Find which **loaded** model fits **thinker → coder → reviewer** by hitting each GPU’s `llama-server` directly (Forge is discovery + judge only).

## Flow

1. **Thinker** — same design/orchestration prompt to every online endpoint.
2. **Gate** — rubric scores + judge model writes **per-model opinions** (strengths, weaknesses, fit, why not best). You review `thinker_opinions.md` before continuing.
3. **Coder** — winner’s thinker outline + coding instructions to every endpoint.
4. **Gate** — `coder_opinions.md`
5. **Reviewer** — best vs worst coder outputs sent to every endpoint.
6. **Gate** — `reviewer_opinions.md` + final scorecard under `var/role_bench/runs/<timestamp>/`

## Run

```bash
cd /codebase/repos/wreckhunter2000-1
export FORGE_URL=http://127.0.0.1:9100          # discovery only
export JUDGE_URL=http://10.0.0.201:5200         # opinion writer (pick your strongest lane)
python3 scripts/role_bench/run_tournament.py
```

Non-interactive (still writes opinions; 3s pause between rounds):

```bash
python3 scripts/role_bench/run_tournament.py --auto
```

Single round:

```bash
python3 scripts/role_bench/run_tournament.py --rounds thinker
```

## Artifacts per run

| File | Contents |
|------|----------|
| `meta.json` | Endpoints, judge URL, timestamp |
| `{round}_submissions.json` | Raw outputs + rubric |
| `{round}_opinions.md` | Human-readable judge opinions — **read before next round** |
| `{round}_opinions.json` | Structured judge JSON + `PROCEED` recommendation |

## GPU UUID sync (optional)

```bash
python3 scripts/role_bench/sync_gpu_uuids.py
# T440 rows need live SSH from forge; until then bus_id + port_base still match.
```

## Customize

- Prompts: `scripts/role_bench/prompts.json`
- Rubric: `scripts/role_bench/grader.py`
- Judge model: change `JUDGE_URL` (e.g. P100 Qwen `:5002` for reviewer-style critiques)
