# Satellite Phase B — RTX 2060 + GTX 1070 only

**Run:** `20260601T163015Z`  
**Packet:** reused `satel_glint_20260601T154030Z/accel_scan_packet.json`  
**Bench ports:** `:5210` (RTX), `:5212` (1070) — avoids mission watchdog replacing `:5200–5202`

## Models (size / quant)

| GPU | Port | Model | VRAM @ run |
|-----|------|-------|------------|
| RTX 2060 SUPER | 5210 | `qwen2.5-coder-14b-instruct-q4_k_m.gguf` (Q4_K_M, ~8.4G) | ~6548 MiB |
| GTX 1070 | 5212 | `Qwen2.5-Coder-7B-Instruct-abliterated-Q8_0.gguf` (Q8_0, ~7.6G) | ~7707 MiB |

**Note:** Both must load with the triple stack stopped; otherwise P106/Phi on `:5201/:5202` steals CUDA memory and 14B OOMs.

## Scores (coder capability @ this size/quant)

| Coder | Grade | Verdict |
|-------|-------|---------|
| **RTX — Qwen2.5-Coder-14B Q4** | **74 / 100** | **Usable tool** — correct sections, sane thresholds from packet, real `process_tile()` body; wrong repo paths (`src/detection/...` vs `cesarops-detection/`). |
| **GTX — Qwen2.5-Coder-7B Q8** | **62 / 100** | **Marginal tool** — all sections + runnable Python, but qualitative cue matrix, invented `wreck_candidates` in example, no fleet module paths. |

### RTX 14B — detail

- **+** Ranked candidate uses TPU score + Movidius fields; FP guards aligned to packet (0.3 score, 0.7 certainty).
- **+** `process_tile()` merges glint + jitter gates and sorts by score/certainty.
- **−** Paths are generic greenfield, not `cesarops-detection/src/...`.
- **−** Assumes `tile_data["tpu_scan"]` wrapper; packet is already flat accel JSON.

### GTX 7B Q8 — detail

- **+** Full script with `process_tile` / `rank_wreck_candidates`; echoes packet constants.
- **+** FP guards reference depth, material, jitter Hz, thermal delta from packet.
- **−** Cue matrix is prose, not tied to `top_detections` grid.
- **−** Embeds duplicate JSON instead of consuming `ACCEL_SCAN_PACKET` only.
- **−** Weaker typing/API fit for production hook-in.

## vs P100 (same prompt, earlier run)

| Model | Grade |
|-------|-------|
| P100 Gemma | 88 |
| P100 Qwen3.6 | 72 |
| RTX Qwen2.5-Coder-14B Q4 | 74 |
| GTX Qwen2.5-Coder-7B Q8 | 62 |

**Takeaway:** At these quants, **14B on 2060 is in the same band as P100 Qwen** for this satellite coder task; **7B Q8 on 1070 is below** but still produces code worth editing—not blank/timeout like the failed `:5200/:5202` attempt under triple-stack contention.

## Reproduce

```bash
sudo systemctl stop cesarops-n8n-watchdog.timer   # brief; restores triple if left stopped
pkill -9 -f llama-server
bash scripts/role_bench/run_satel_phase_b_rtx1070_only.sh var/role_bench/satel_glint_20260601T154030Z
```

Outputs: `coder_RTX-2060.md`, `coder_GTX-1070.md` in this directory.
