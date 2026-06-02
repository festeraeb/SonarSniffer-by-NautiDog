# P106 PAMP bootstrap — Phi-3 vs Gemma-4-E4B

**Port:** `:5201` (P106, `CUDA2`) · **Prompt:** golden PAMP classify + execute variant

| Model | VRAM (P106) | PAMP classify output | Latency |
|-------|-------------|----------------------|---------|
| **Phi-3-mini-4k Q4** | ~4070 MiB | `{"task_type":"architecture"}` only | ~0.6s |
| **Gemma-4-E4B Q4** | ~3656 MiB | `{"parallel":["expert_a"],"serial_after":["expert_b"],"task_type":"architecture"}` | ~0.6s |

**Execute prompt** (`cargo test on integrate module`):

- Gemma: `{"task_type":"execute","parallel":[],"serial_after":[]}` (~0.7s) ✓

## Recommendation

**Prefer Gemma-4-E4B** for PAMP `:5201` — follows JSON schema, still fast, slightly less VRAM than Phi.

```bash
MODEL_PAMP=/mnt/t440/models/gemma-4-E4B-it-Q4_K_M.gguf \
  bash scripts/cesarops2_pamp_p106.sh start
```

Phi remains fine if you want minimal VRAM churn and only need `task_type`.
