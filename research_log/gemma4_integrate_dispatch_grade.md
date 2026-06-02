# Gemma-4-26B-MoE integrate dispatch — grade report

**Date:** 2026-05-26  
**Model:** `Gemma-4-26B-MoE-IQ4_XS.gguf` on P100 #0 (`:5001`, CUDA0)  
**Job:** `scripts/p100_dispatch_integrate.py` — 80 integrate + 7 live_reference (reference on Qwen `:5002` at job start)

## Overall grade: **B+** (87/100)

| Dimension | Score | Notes |
|-----------|-------|--------|
| Completion | 94/100 | 75/80 integrate reports usable; 5 empty (header-only) |
| Format compliance | 96/100 | Required sections present when non-empty |
| Technical depth | 85/100 | Concrete paths, deps, Windows-path risks; some generic steps |
| Verdict calibration | 78/100 | 73× `PORT_TO_PIPELINES`; rarely `ARCHIVE_STUB` / `NEEDS_HUMAN` |
| Fleet awareness | 88/100 | Often mentions Forge, CuPy, T440/M2200, container paths |

## Verdict distribution (86 reports total)

| Verdict | Count |
|---------|-------|
| PORT_TO_PIPELINES | 73 |
| MERGE_INTO_LIVE | 3 |
| ARCHIVE_STUB | 2 |
| KEEP_LIVE | 1 |
| Missing / empty | 7 |

## Failures (re-run recommended)

Integrate tasks with **no body** (only `# path` header):

- `int__anchor_lock_display.md`
- `int__cleanup_and_organize.md`
- `int__integrated_forensic_scan.md`
- `int__three_tile_offset_analysis.md`
- `int__andaste_geometry_test.md`

Likely cause: long input truncation + busy server / empty completion at end of 80-file serial queue.

## Strong examples (A tier)

- `int__global_controls.md` — sensible `scanner_ops.py` split, VRAM risk called out
- `int__cesarops_engine.md` — CuPy/TPU/SQLite → fleet schema mapping
- `int__test_pipeline.md` — pytest + CI GPU caveats
- `int__analyze_fuel_leaks.md` — spectral indices + upstream scanner dependency

## Reference diffs (Qwen on `:5002`, not Gemma)

7 `ref__*` tasks at job start: **4 excellent**, **3 empty/too short** (`mag_data_pipeline`, `cmr_search`, `universal_downloader`). Best: `ref__adaptive_background_scan`, `ref__advanced_bag_scanner_runner`.

## Recommendation

1. Re-dispatch the **5 failed integrate** paths only (`MAX_INTEGRATE_PER_GPU=5` one-off).
2. Treat `PORT_TO_PIPELINES` as **proposal** — human triage before any port (many laptopdump scripts are one-off experiments).
3. Prioritize high-value ports: `cesarops_engine`, `global_controls`, `test_pipeline`, `ai_director` (p1001).

**Outputs:** `integrate_out/p1000/` (40), `integrate_out/p1001/` (46), `integrate_out/dispatch_summary.json`
