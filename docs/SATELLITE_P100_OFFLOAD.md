# Satellite math on twin P100s (vs T440 Xeon)

Pascal P100s are weak for large LLM inference but strong for **batched f32/f64 linear algebra** on megapixel rasters — often faster than dual Xeons when the work is FFTs, convolutions, and per-pixel z-scoring at scale.

## Keep on Xeon (orchestration, I/O)

| Workload | Why CPU |
|----------|---------|
| Open-Meteo / `env_conditions` | API + small arrays |
| STAC search + `download_cog_chip` | Network + GDAL decode (I/O bound) |
| `validate_gt`, fusion scoring | Small JSON / point logic |
| Mission stage machine | `sat-run` coordinator |
| Qwen3.6 `:5010` think/polish | MoE needs RAM; already on CPU by design |

## Move to P100 (compute-heavy)

| Module | Math | Split strategy |
|--------|------|----------------|
| `phase_corr.rs` | 2D FFT cross-power | Batch all scene pairs per wreck chip on P100-0 |
| `temporal.rs` | LOO + `baseline_residual` + align | Scene-parallel: even scenes → GPU0, odd → GPU1 |
| `poc.rs` | Sobel, uniform_filter, z-score, NMS | `run_poc_local` scene list split across GPUs |
| `bathymetry_map.rs` | Multi-pass depth fuse + relief gradient | Per-scene depth maps on GPU, fuse on CPU |
| `overlay_grid.rs` | Marker search / alignment windows | Optional P100 for large grids |
| `magnetic.rs` | VDR FFT (`rustfft`) | If mag chips run on satellite path |

## Not on P100

- Forge / llama-server (use RTX for Mixtral or CPU `:5010` only)
- Coral TPU / Movidius (`:8092`, `:8180`) — fixed accelerators
- BAG HDF5 scan — separate `cesarops-bag-scan` job

## Implementation phases

### Phase 0 (now)
- `RAYON_NUM_THREADS` on Xeon for `run_poc_local` / temporal local
- Document env in mission spec (no CUDA in `cesarops-satellite` yet)

### Phase 1 (next code)
- Env `SAT_P100_SPLIT=1`: shell wrapper runs two `sat-run` stage workers with `CUDA_VISIBLE_DEVICES=0|1` and scene-id shard — **only after** GDAL chip decode stays on CPU and GPU gets numeric arrays (NPZ sidecar or shared memory)

### Phase 2
- Optional `cuda` / `cudarc` feature in `phase_corr` for batched FFT
- Or offload to existing `nauticuvs` GPU paths where curvelet/FFT already exists

### Phase 3
- Turn off Forge during `run_detection_path.sh` (already default)
- Cron: detection path nightly; Forge only for ad-hoc spec

## Operator layout (T440)

```bash
bash scripts/forge_llm_watchdog_off.sh     # while running detection + manual coding
bash scripts/t440_dual_lane_layout.sh stop # free VRAM if sat GPU phase needs P100
# Future:
# SAT_P100_DEVICES=0,1 bash scripts/role_bench/run_detection_path.sh
```

**Do not** run Gemma + Qwen14 on P100 while saturating the same cards with satellite FFT — pick **coding session** vs **detection batch** by time slot.

## Why this beats Forge for satellite

| Approach | Outcome |
|----------|---------|
| Forge Lane A/B on P100 | Slow, hallucinated knobs, no `cargo test` |
| Cursor + `sat-run` | Repo-correct, measurable @ preserve GT |
| P100 numeric offload | Faster stacks, same Rust source of truth |

Gemini reviews **physics and tuning**; P100 executes **math**; Xeon **orchestrates**.
