# Satellite glint coder round — grades (P106 excluded)

**Accel packet:** `accel_scan_packet.json`  
TPU: 1 glint peak (CPU stub, `used_tpu: false`) · Movidius: `ferrous_composite`, certainty 0.738, `confirmed_structure`

| Model | Port | Score | Verdict |
|-------|------|-------|---------|
| **P100 Gemma** | 5001 | **88** | **Winner** |
| **P100 Qwen** | 5002 | **72** | Strong matrix; bloated |
| RTX 2060 | 5200 | — | Offline / connection reset during load |
| GTX 1070 | 5202/5203 | — | Offline |

## P100 Gemma — 88 (winner)

**Strengths:** All four sections; uses every packet field; sensible FP guards (jitter Hz, thermal Δ, cross-check TPU+Movidius); `process_tile()` is short and shippable under `cesarops-detection/`.

**Weaknesses:** Invents “dark/clear water” values not in packet; `/data/missions/` paths are generic not repo-real.

**Why best for this lane:** Best **coder** behavior — turns accel telemetry into ranked wreck + guards + code without drowning in chain-of-thought.

## P100 Qwen — 72

**Strengths:** Rich cue matrix tied to JSON paths; good Great Lakes depth guard (100–200 ft); modular sketch (`cue_matrix.py`, `guards.py`).

**Weaknesses:** Huge reasoning dump after answer; table truncated; over-engineered for a single-tile packet.

## RTX / 1070

Re-run when Qwen2.5-Coder / Phi ports are stable:

```bash
bash scripts/role_bench/run_satel_glint_coder_round.sh
```

## Pipeline recommendation

1. **TPU** — glint / bright-pixel repeat scan (tile batches).  
2. **Movidius** — jitter + material at fixed lat/lon.  
3. **Fuse** → `accel_scan_packet.json` per tile.  
4. **LLM coders** — P100 #1 UX copy + P100 #2 science QA; c2 RTX/1070 for parallel implementation variants.  
5. **P106** — keep off hot path until larger models stable.
