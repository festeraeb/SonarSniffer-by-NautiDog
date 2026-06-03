# Detection path — completion plan (Cursor + Gemini, Forge optional)

**Goal:** Close the Straits wreck-detection loop using **preserve GT**, **sat-run**, and **accel corroboration**. Forge LLM lanes are advisory only; they shrink over time as **twin P100s** take math-heavy satellite work.

## End state (target architecture)

```text
                    ┌─────────────────────────────────────┐
                    │  Cursor implementor + Gemini review │
                    │  (spec, tuning, collab bus)         │
                    └─────────────────┬───────────────────┘
                                      │
     ┌────────────────────────────────┼────────────────────────────────┐
     │                                │                                │
     v                                v                                v
┌─────────────┐              ┌─────────────────┐              ┌──────────────────┐
│ T440 Xeon   │              │ T440 P100 ×2     │              │ c2 RTX (optional) │
│ CPU         │              │ satellite math   │              │ Mixtral review    │
│ :5010 Qwen  │              │ :5001 scene A    │              │ (sunset → off)    │
│ weather API │              │ :5002 scene B    │              └──────────────────┘
│ STAC I/O    │              │ FFT / z / fuse   │
│ validate_gt │              │ bathy_map LOO    │
└─────────────┘              └─────────────────┘
     │                                │
     └──────────── sat-run ────────────┘
                    │
     ┌──────────────┼──────────────┐
     v              v              v
  TPU :8092    Movidius :8180   BAG scan (optional)
```

## Detection path checklist

| Step | Owner | Artifact | Status |
|------|-------|----------|--------|
| 1. Preserve GT in bbox | Cursor | `known_wrecks_straits.json`, `gt_min_confidence: 1.0` | Done |
| 2. Tool stack brief for Gemini | Cursor | `docs/TOOL_STACK_GEMINI_BRIEF.md` | Done |
| 3. Gemini tool rankings + tuning order | Gemini | `inbox/gemini_ack_round5.json` (paste round 5) | **Pending** |
| 4. Full detection run (no Forge) | Cursor | `run_detection_path.sh` → `detection_path_report.json` | Ready |
| 5. Tune gates from Gemini + metrics | Cursor | `temporal.rs`, `fusion.rs`, `poc.rs` | In progress |
| 6. SAR GDAL RTC open | Cursor | `sar.rs` / path to RTC tif | Blocked |
| 7. Glint accel full pass | Cursor | `straits_glint_accel/accel_scan_packet.json` | Smoke OK |
| 8. Fuse accel + persistence | Cursor | `fuse_glint_accel.py` | Done |
| 9. Material fusion weights (steel/wood) | Cursor | `fusion.rs` + OpenMemory | Queued |
| 10. P100 offload phase 1 | Cursor | See `SATELLITE_P100_OFFLOAD.md` | Planned |

## Run without Forge

```bash
# On host with NFS tiles + built sat-run
bash scripts/forge_llm_watchdog_off.sh          # keep :5002 on Qwen14 if coding later
bash scripts/role_bench/run_detection_path.sh
```

Outputs under `/data/cesarops/satellite_data/detection_runs/straits_local_2024/`:
- `detection_path_report.json`
- `validation_report.json`
- `temporal_stack/glint_persistence_map.json`
- `prove_tools_glint_fused.json` (if accel run)

## Gemini collab (detection-focused)

Paste: `var/forge_collab/outbox/GEMINI_DETECTION_PATH_ROUND6.md`

Ask: tool rankings, missing math, tuning order, and **which stages should move to P100 first** vs stay on Xeon.

## Forge sunset (phased)

| Phase | Forge role | Satellite execution |
|-------|------------|---------------------|
| **Now** | Optional spec/review; often too slow on Pascal | Cursor + `sat-run` + `cargo test` |
| **Next** | Off during detection runs (`forge_llm_watchdog_off`) | P100: per-scene POC + temporal chips |
| **Later** | n8n mission dispatch only | P100: FFT phase-corr batches, bathy fuse |
| **End** | No LLM on hot path | Full `sat-run` on Xeon orchestration + P100 compute |

LLM coders do not replace **compile-test-sat-run** for repo-correct Rust; they inform knobs and physics only.

## Success criteria (Straits)

- `validate_gt`: Cedarville (steel) and ≥2 preserve wrecks show non-`no_data` with plausible concept
- Temporal: ≥1 candidate within 500 m of a preserve wreck above dynamic gate
- Glint+accel: corroborated flag near Burns proxy / preserve wood targets
- Report JSON documents per-tool hits for Gemini round 6 merge
