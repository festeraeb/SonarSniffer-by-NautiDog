# Spec draft bench (local fallback, RTX + 1070)

Tests whether a **local** llama-server can turn operator chat into **OperatorSpec JSON** before Forge runs tools. Inference stays off P100s so satellite jobs can keep `:5001` / `:5002` on T440.

## Hardware policy

| Port | GPU | Role (unified preset) | Typical model |
|------|-----|------------------------|---------------|
| 5200 | RTX 2060 SUPER | draft / MoE | Gemma-4-E4B Q4, or Qwen MoE |
| 5203 | GTX 1070 | thinker / spec refine | ZAYA1-8B Q4 (Vulkan) |

Bench **only** talks to these ports unless you pass `--allow-p100`.

## Quick run (on cesarops2)

```bash
cd /data/codebase/repos/wreckhunter2000-1
bash scripts/role_bench/run_spec_draft_bench.sh
```

Artifacts: `var/role_bench/spec_draft/<timestamp>/` (`results.json`, `report.md`, per-fixture JSON).

## Compare models

1. Load model A on a port (restart llama-server).
2. Run bench → note average score for that port.
3. Swap GGUF, restart, re-run.

Examples:

```bash
# ZAYA on 1070 (unified default)
bash scripts/zaya/start_zaya_1070.sh

# RTX draft slot
FLEET_UNIFIED=1 MODEL_RTX=/mnt/t440/models/gemma-4-E4B-it-Q4_K_M.gguf \
  bash scripts/cesarops2_fleet_roles.sh start

bash scripts/role_bench/run_spec_draft_bench.sh
```

Single fixture:

```bash
python3 scripts/role_bench/run_spec_draft_bench.py --fixture forge_corrector_cap
```

## Scoring

`spec_grader.py` checks:

- Valid JSON + required OperatorSpec fields
- Actionable `do[]` with success criteria
- `needs_user_approval: true`
- No `/path/to/` placeholders
- Optional: mentions P100 / T440 constraint awareness

See `operator_spec_example.json` for the target shape.

## Files

- `spec_draft_prompts.json` — system + user template
- `spec_draft_fixtures.json` — three realistic operator prompts
- `run_spec_draft_bench.py` — runner
- `run_spec_draft_bench.sh` — ensures :5200/:5203 are up

## Models that usually fit

**1070 (8 GB):** ZAYA1-8B Q4_K_M, Phi-3-mini Q4, Qwen2.5-3B Q8  
**RTX 2060 SUPER (8 GB):** Gemma-4-E4B Q4, Qwen2.5-Coder-7B (partial `-ngl`), Llama-3.2-3B

Avoid loading 14B+ full-GPU on both cards at once.

## Next step (Forge integration)

When a port wins the bench, point `SPEC_DRAFT_URL` (future) at that endpoint for `POST /spec/draft`; keep human `approve` before `/send`.

Gemini Spec Thinker API key: `scripts/credentials.gemini.local.sh` (gitignored), loaded via `scripts/credentials.sh`.

## Forge wiring (done)

| Endpoint | Role |
|----------|------|
| `POST /spec/draft` | Run spec thinker (Gemini `auto` or `local_only` in body) |
| `POST /spec/approve` | Human gate before `/send` |
| `GET /spec/status` | Pending spec state |
| `POST /spec/clear` | Drop pending spec |

Config: `cesarops-forge-v2/cluster_config.toml` → `[spec_draft]`. UI: **SPEC** / **APPROVE** in Forge header.

```bash
bash scripts/forge_spec_pipeline.sh "your operator intent"
```
