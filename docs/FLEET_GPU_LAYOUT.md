# Fleet GPU layout (2026-06-01)

Benchmark-backed roles. **Unload before load** on every swap (`pkill` port or `POST /orchestrator/swap`).

## Topology

| GPU | VRAM | Model (on disk) | Port | Role |
|-----|------|-----------------|------|------|
| **RTX 2060 Super** | 8 GB | **Gemma-4-E4B-it-Q4_K_M** | `:5200` | Fast **dispatcher / thinker** — structured plans (~84/100); **not** DeepSeek-R1-7B |
| **Tesla P100 #0** | 16 GB | **Qwen3.6-35B-A3B-Q4_K_M** (or MXFP4) | `:5002` | High-capacity **worker** (review, integrate, science) |
| **Tesla P100 #1** | 16 GB | **Gemma-4-26B-MoE-IQ4_XS** | `:5001` | High-capacity **worker** (coder / UX) |
| **GTX 1070** | 8 GB | **Qwen2.5-Coder-7B** Q8 (Q4 if added) | `:5202` | Focused **fast scripting** coder |
| **T440 CPU** | RAM | **Qwen3-Coder-Next-IQ3_M** | `:5010` | **Polisher only** — after coders fail / still broken |
| **P106** | 6 GB | **Gemma-4-E4B Q4** (Phi-3 alt) | `:5201` | **PAMP bootstrap** / `thinker_fast` — fast route JSON (not R1-7B) |

### P106 PAMP lane

- Script: `scripts/cesarops2_pamp_p106.sh` (Gemma E4B default; `MODEL_PAMP=.../Phi-3-mini-4k-instruct-Q4_K_M.gguf` for alt). Compare: `var/role_bench/pamp_p106_phi_vs_gemma_e4b.md`.
- Forge `bootstrap` + `thinker_fast` → `:5201`; golden suite label **PAMP** / baseline `pamp_moe_test`.
- Smoke: `bash scripts/cesarops2_pamp_p106.sh smoke`

### Why not DeepSeek-R1-Distill-7B on RTX?

- Thinker round: weak / wrong format on P106 (`manual_P106-R1-7B.md`).
- `lessons_learned`: R1-7B leaks thinking tags and unusable “design” output unless narrowly scoped.
- **Use Gemma-4-E4B** on RTX instead (same plan prompt **84/100** on 1070; fits full GPU on 2060).

### Optional RTX download (if you want a coder-lean dispatcher)

- `Qwen2.5-Coder-3B-Instruct` Q4 (~2–6 GB) — faster tool-routing, less narrative than E4B.

## Forge pipeline order

1. **Thinker** — RTX `:5200` (Gemma E4B) delegates **distinct** tasks per worker (not one shared task).
2. **Coders** (parallel) — P100 Gemma `:5001`, P100 Qwen `:5002`, 1070 Qwen2.5-Coder `:5202`.
3. **Cross-review** — Gemma reviews Qwen’s artifact; Qwen reviews Gemma’s (parallel).
4. **1070 review** — whichever P100 reviewer finishes first takes 1070 output.
5. **Polisher** — T440 CPU `:5010` only for failed review / coder / empty handoff.

Bench runner: `bash scripts/role_bench/run_fleet_hetero_pipeline.sh`  
Forge preset: `bash scripts/forge_apply_fleet_hetero_cross_review.sh` (`fleet-hetero-cross-review`)

**Note:** Forge `/send` preset `fleet-hetero-20260601` uses `parallel_dual_coders` (Gemma + 1070 as dual coders). That is **not** the same as P100 cross-review — use the bench script for Gemma↔Qwen review.

Set env for integrate scripts:

```bash
export POLISHER_URL=http://127.0.0.1:5010/v1/chat/completions   # on T440
export FORGE_CORRECTOR_FALLBACK="$POLISHER_URL"
```

## GPU slot watchdog (dynamic restore)

When n8n runs `mission_service_watchdog`, GPU recovery uses **last heartbeat** per port — not the triple-stack preset.

See [GPU_SLOT_WATCHDOG.md](./GPU_SLOT_WATCHDOG.md).

## Start scripts (on each host)

| Host | Command |
|------|---------|
| **T440** | `bash scripts/p100_gemma_r1_dual.sh start` |
| **T440** | `bash scripts/t440_polisher_coder_next_cpu.sh start` |
| **cesarops2** | `bash scripts/cesarops2_fleet_roles.sh start` |

## Routing preset

Apply: `bash scripts/forge_apply_fleet_hetero_layout.sh`

Preset id: `fleet-hetero-20260601` in `cluster_config.toml`.
