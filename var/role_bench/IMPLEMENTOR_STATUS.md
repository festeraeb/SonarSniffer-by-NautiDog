# Implementor run — live status

## Operating mode

- **Parallel jobs** — one pipeline process per `JOB_INDEX` (`scripts/run_implementor_parallel.sh`).
- **Code first** — ship Rust fixes between rounds; LLM output is advisory.
- **Learnings** — append-only `var/role_bench/implementor_learnings.jsonl` via `implementor_tracker.py`.

## Detection path (active — Cursor + Gemini, Forge not on hot path)

- **Run:** `bash scripts/role_bench/run_detection_path.sh` → `detection_path_report.json`
- **Docs:** `docs/DETECTION_PATH_ROADMAP.md`, `docs/SATELLITE_P100_OFFLOAD.md`
- **Gemini:** paste `var/forge_collab/outbox/GEMINI_DETECTION_PATH_ROUND6.md`
- **Later:** shut Forge during detection; P100s for FFT / scene-parallel POC / LOO (not LLM)

## Round 1 / fleet queue (Forge advisory only)

**Code shipped (implementor, no LLM wait):**
- `poc.rs` — Z cap 4.0, edge margin 3; **TOOL 5** `concept_glint_roughness` (417 POC cands on last `sat-run`)
- `temporal.rs` — LOO persistence + phase-corr alignment + **glint LOO** map
- **Glint accel (TPU + Movidius)** — `docs/GLINT_ACCEL_STACK.md`, `run_straits_glint_accel.sh`, `fuse_glint_accel.py`
- **Tool proving** — `prove_tools_preserve_gt.sh` + `docs/TOOL_STACK_GEMINI_BRIEF.md` (weather/scene pick, SDB `bathymetry_map.rs`, aeromag **separate**, drift sidecar)
- **Gemini round 5** — paste `var/forge_collab/outbox/GEMINI_TOOL_STACK_ROUND5.md`
- `sar.rs` — **TOOL 1** `extract_sar_anomalies_local` + `sar_local` mission stage (GDAL open on RTC tif still failing — see learnings)
- Mission `straits_local_run.json` — stages include `sar_local`

**sat-run (2026-06-03):** temporal persist ~0.06–0.08 @ Cedarville/Burns (gate 0.3 → 0 candidates). SAR stage: `GDALOpenEx` NULL on RTC GeoTIFF.

**Learnings log:** `var/role_bench/implementor_learnings.jsonl`

## Round 1 detail

| Item | Status |
|------|--------|
| `poc.rs` Z cap + glint concept | **Shipped** |
| `sar.rs` extract + mission stage | **Shipped**; runtime GDAL fix TBD |
| `phase_corr` + LOO temporal | **Shipped**; metrics below gate |
| Parallel LLM jobs `implementor_20260603T043018Z` | Stalled/slow — rerun when T440 layout up |
| Fleet tools 02–04 (thermal, SWOT, ICESat-2) | **Queued** in `dual_lane_jobs_fleet_tools.json` |
| Gemini ↔ Cursor collab | **Complete (8/8)** — prove tools vs preserve GT; BAG not on sat-run path |

## Experiments log

```bash
python3 scripts/role_bench/implementor_tracker.py summary
```

## Job 03 — phase correlation (friend JSON)

- **Queue file:** `scripts/role_bench/dual_lane_job_03_phase_correlation.json`
- **Template for new jobs:** `scripts/role_bench/implementor_experiment_template.json`
- **Rust (implementor-shipped):** `phase_corr.rs`, `engine_error.rs`, hook in `temporal.rs`
- **Run Lane A job:**
  ```bash
  JOBS_FILE=scripts/role_bench/dual_lane_job_03_phase_correlation.json JOB_INDEX=0 \
  QWEN_URL=http://127.0.0.1:5010 bash scripts/run_dual_lane_forge_pipeline.sh
  ```
- **Metric gate:** after `sat-run`, log `phase_correlation_fft` with persistence peak @ Burns/Cedarville

## Next tightenings to try (record worked=true/false after each)

1. `FORGE_MEMORY=1` — nautivecs snippet in plan/polish (already on).
2. `PIPELINE_CTX_CHARS=12000` — avoid Qwen polish 400.
3. Route POC knob tasks to **Lane A only**; fusion/GDAL to Lane B.
4. Post-round `sat-run` on NFS straits mission + GPS check Cedarville/Burns.

## Commands

```bash
# Parallel bench
JOB_INDICES="0 1" JOBS_FILE=scripts/role_bench/dual_lane_jobs_round1.json \
  QWEN_URL=http://127.0.0.1:5010 bash scripts/run_implementor_parallel.sh

# Fleet tools queue (after round1)
JOB_INDICES="0 1 2" JOBS_FILE=scripts/role_bench/dual_lane_jobs_fleet_tools.json \
  bash scripts/run_implementor_parallel.sh
```
