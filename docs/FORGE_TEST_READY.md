# Forge cluster — test readiness (2026-05-24)

## Update: per-model corrector tuning + routing evidence (2026-05-31)

### Manual role scorecard (human-scored, latest tuning cycle)

Scoring rubric (manual):
- Concrete commands/paths (0-35)
- Correctness for stated n8n issue (0-35)
- Rollback + verification quality (0-20)
- No placeholders/meta scaffolding (0-10)

Run A artifacts:
- `/tmp/forge_role_monitor_tuned-pass_same-model_20260531T201843Z/summary.json`
- `/tmp/forge_role_monitor_tuned-pass2_specialized_20260531T202136Z/summary.json`

Run B artifacts (after monitor-rubric hardening + timeout guard):
- `/tmp/forge_role_monitor_tuned-pass4_same-model_20260531T202916Z/summary.json`
- `/tmp/forge_role_monitor_tuned-pass4_specialized_20260531T203131Z/summary.json`

Manual scores:

| Run | Role | Manual score | Notes |
|-----|------|--------------|-------|
| A same-model | thinker | 28/100 | Generic steps, weak task grounding, little concrete path evidence |
| A same-model | coder | 31/100 | Placeholder-heavy (`your-repo`, generic clone/test flow) |
| A same-model | reviewer | 24/100 | Template checklist language, no concrete evidence audit |
| A specialized | thinker | 36/100 | Better structure, still meta/think-style and partial specificity |
| A specialized | coder | 33/100 | Meta scaffolding and generic edits, still placeholder-prone |
| A specialized | reviewer | 30/100 | Requirement restatement more than evidence-backed review |
| B same-model | thinker | 0/100 | Dispatch timeout/no response |
| B same-model | coder | 0/100 | Dispatch timeout/no response |
| B same-model | reviewer | 0/100 | Dispatch timeout/no response |
| B specialized | thinker | 0/100 | Dispatch timeout/no response |
| B specialized | coder | 37/100 | Concrete commands present but placeholder paths and weak rollback |
| B specialized | reviewer | 44/100 | Better rollback/verify mention, still generic and weakly evidential |

What changed in this cycle:
- `scripts/forge_role_monitor.sh`:
	- stronger anti-placeholder and anti-meta scoring penalties
	- explicit feature flags in metrics (`has_commands`, `has_path`, `has_placeholder`, etc.)
	- hard probe timeout guard via `timeout` + tighter curl connect/max-time
	- stricter role probes requiring concrete command/path/rollback/verify output format
- `cesarops-forge-v2/src/prompts.rs`:
	- system + thinker prompt contracts now explicitly forbid `<think>` tags and meta scaffolding text

Cycle conclusion:
- Tool-tuning effect is mostly exhausted for this pass without infra stabilization on endpoint `:5200`.
- Primary blocker is transport/reliability (timeouts) plus persistent generic output behavior under direct probe.
- Keep manual scoring loop active, but prioritize endpoint reliability before interpreting quality deltas.

### Newly completed

| Task | Status | Evidence |
|------|--------|----------|
| Wire corrector behavior to model-aware presets (by endpoint model identity) | Completed | `cesarops-forge-v2/src/loop_engine.rs` now resolves model from endpoint and applies preset strength to prompt/temperature/token budget |
| Seed per-model corrector presets in config | Completed | `cesarops-forge-v2/cluster_config.toml` now includes `[corrector_preset.*]` and `[[corrector_preset_match]]` blocks |
| Add per-model scorecard helper | Completed | `scripts/forge_corrector_tune_report.sh` |

### Recommended starting lineup from downloaded bundle

- thinker: `DeepSeek-R1-Distill-Qwen-14B-IQ4_XS.gguf`
- coder: `qwen2.5-coder-7b-instruct-q4_k_m.gguf`
- reviewer+corrector: `gemma-2-9b-it-IQ4_XS.gguf`
- validator fast: `Qwen3-8B-Q4_K_M.gguf`
- validator fallback: `Phi-4-mini-instruct.Q4_K_M.gguf`

### Role monitor evidence (latest stable artifacts)

Source files:

- `/tmp/role_same_thinker.json`
- `/tmp/role_same_coder.json`
- `/tmp/role_same_reviewer.json`
- `/tmp/role_spec_thinker.json`
- `/tmp/role_spec_coder.json`
- `/tmp/role_spec_reviewer.json`

Observed outcomes:

- All six probes returned `error=null`.
- All six probes resolved `endpoint_used=http://10.0.0.201:5202`.
- same-model chars: thinker `354`, coder `302`, reviewer `369`.
- specialized chars: thinker `415`, coder `249`, reviewer `228`.

Interpretation:

- Specialized routing is not yet producing reliable endpoint separation in these probes.
- Endpoint separation must be verified first, then score deltas can be trusted.

### Low-score A/B routing status (P1/P2)

Completed evidence:

- `/tmp/P1-shell-errors-spec.log`: best score `65` (Partial), mostly endpoint `5202`.
- `/tmp/P2-thinker-latency-spec.log`: best score `50` (Blocked), endpoint `5202`.

Interrupted/incomplete evidence:

- `/tmp/P1-shell-errors-same.log`: header only.
- `/tmp/P2-thinker-latency-same.log`: header only.

### Next steps

1. Enforce same-model vs specialized endpoint split before A/B scoring.
2. Re-run P1/P2 with hard timeouts and guaranteed summary artifact emission.
3. Gather scorecard data and iterate preset matcher thresholds via `forge_corrector_tune_report.sh`.

## Operational task list (2026-05-31)

### Completed now

| Task | Status | Evidence |
|------|--------|----------|
| Stabilize `/cluster/test/dispatch` response path (no hang) | Completed | Dispatch returns JSON for B5/B3/B7 with result objects |
| Tune dispatch probes for bounded runtime | Completed | Direct probe mode uses short timeout + reduced token budget |
| Redeploy Forge after fix | Completed | `cargo build --release` + service restart succeeded |
| Validate fleet route health after Forge changes | Completed | `fleet-route-health` summary `ok=12 warn=1 fail=0` |
| Run P1 n8n-dedupe task through Forge and grade output | Completed (85/100) | `/cluster/test/dispatch` custom task returned actionable command sequence and rollback guidance |

### Next important tasks

| Priority | Task | Status |
|----------|------|--------|
| P0 | Forge-to-Forge state sync (jobs, loaded models, run history) between T440 Cloudflare-facing Forge and cesarops2 Forge so remote UI shows the same active runs | Added (design + wiring pending) |
| P1 | Fix shell-invocation errors in n8n executeCommand jobs (`Bad substitution`, wrong script path) | Next (current grade 65/100, Partial) |
| P1 | Improve remote thinker latency/reliability on `:5200` for richer dispatch outputs | Next (current grade 65/100, Partial) |
| P2 | Execute DB dedupe changes with backup+rollback in maintenance window, then re-grade | Next (planning complete; execution pending) |

### Forge task run results (2026-05-31)

Artifacts:
- `/tmp/forge_task_runs_20260531T181425Z/tasks.json`
- `/tmp/forge_task_runs_20260531T181425Z/forge_custom_results.json`
- `/tmp/forge_task_runs_20260531T181425Z/graded_summary.json`

| Task label | Forge status | Grade | Notes |
|------------|--------------|-------|-------|
| P1-n8n-dedupe | Completed | 85/100 | Correct and actionable; includes backup/rollback focus |
| P1-shell-errors | Partial | 65/100 | Useful direction, but needs more concrete file-edit commands |
| P2-thinker-latency | Partial | 65/100 | Safe plan provided, but tuning checks need tighter thresholds |

### Forge task run + grading protocol

For each Next task:
1. Run via Forge (`/cluster/agent/run` or tuned dispatch endpoint).
2. Capture JSON output artifact under `/tmp/forge_task_runs_*`.
3. Grade on:
	- correctness (0-40)
	- actionability (0-30)
	- safety/rollback awareness (0-20)
	- brevity/clarity (0-10)
4. Mark task `Completed`, `Partial`, or `Blocked` with grade.

## Status: ready for UI big task

Smoke and golden tests were run from T440 against Forge `:9100`.

| Check | Result |
|-------|--------|
| Forge `:9100` | OK |
| n8n `:5678` | OK (fleet-ops + pamp-route) |
| cesarops2 thinker `:5200` | OK |
| cesarops2 MTP `:5571` | OK |
| T440 coder `:5001` | OK (Gemma MoE) |
| T440 reviewer `:5002` | OK (Qwen MTP) |
| nautivecs `:5003` | OK (`target/release/nautivecs-cli serve`) |

## Applied routing

- Preset: **`golden-test-b5`** (saved in `routing_state.json` / `mode_state.json`)
- Fleet sync via n8n: **`sync_llm_endpoints`** → NFS queue on cesarops2

## Test results (automated)

| Test | Result |
|------|--------|
| Smoke 1a — agent `:5200` | PASS |
| Smoke 1c — B5 dispatch | PASS (~10s) |
| Golden B5 | PASS |
| Golden B3 | PASS |
| Golden B7 | PASS |
| PAMP shadow | PASS (expert_plan returned) |

Artifacts: `/tmp/smoke-1a.json`, `/tmp/smoke-1c.json`, `/tmp/forge-golden-results.json`

## Your big task (Forge UI)

1. Open **http://127.0.0.1:9100/cluster** (or main Forge chat).
2. Confirm routing preset **golden-test-b5** or **pamp-moe-predict** if you want PAMP orchestration.
3. Send your task in the chat — `/send` uses full loop (corrector, vectors when `inject_vectors` on).
4. For PAMP-heavy work: set orchestration **tools_backend = n8n**, **pamp_shadow = false** in cluster panel.

## Quick health (before a long UI run)

```bash
curl -sf http://127.0.0.1:9100/health
curl -sf http://127.0.0.1:5001/v1/models
curl -sf http://10.0.0.201:5200/v1/models
curl -sf http://127.0.0.1:5003/health
```

## Start services after reboot

```bash
bash scripts/start_n8n.sh
bash scripts/p100_cycle.sh restore
/codebase/repos/wreckhunter2000-1/target/release/nautivecs-cli serve --port 5003 &
# cesarops2: scripts/cesarops2_research_lab.sh start
```

## Fixes applied this session

- nautivecs binary path: workspace `target/release/nautivecs-cli`
- n8n PAMP workflow: valid `JSON.stringify` bodies; shadow fast path; DB patch for workflow `KYHoNSz9XbzswntQ`
- `finish_worthy_stubs.sh`: correct nautivecs-cli path
