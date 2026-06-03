# CESAROPS Forge Runtime — System Profile

Canonical split between **algorithmic truth** (nautivecs + Nauticuvs), **operational guardrails** (OpenMemory), and **dual-lane LLM execution**.

## 1. Core infrastructure

| Component | Role | Context scope |
|-----------|------|----------------|
| **nautivecs** | Source of truth for math blueprints, proofs, `f64` trait boundaries | Lee Secchi, Planck thermal, Alpers Bragg, FFT/wavelet/curvelet maps → Rust |
| **OpenMemory** (`:8765`) | Non-volatile guardrails, bugs, calibrations, hardware limits | Z-score caps, temporal 0.08 gate, fusion weights, T440 NUMA, VRAM caps |
| **Nauticuvs** | `f64` FFWT curvelet filter (library, not RAG) | Anisotropic wakes, plume edges, internal waves; ignore isotropic noise |

Ingest scripts: `scripts/ingest_forge_runtime_context.sh` → nautivecs `:5003/add` + OpenMemory when up.

## 2. Multi-lane protocol

```
[ LANE A — Satellite physics & signal processing ]
  Mixtral 8x7B (Thinker) → Gemma 4 MoE (Coder, T440 :5001) → Mixtral (Polisher)
  Tasks: curvelets, Planck inversions, sub-pixel coregistration, POC concepts

[ LANE B — Architecture & pipeline integration ]
  Qwen3 MoE (Thinker, T440 :5002) → Qwen2.5-Coder-14B (Coder, c2 :5203) → Qwen (Polisher)
  Tasks: SignalBundle, serde, GDAL/NetCDF ingest, fusion.rs, mission stages
```

**Physical layout (dual-lane):**

| Host | Port | GPU | Role |
|------|------|-----|------|
| c2 | 5200 | RTX | Lane A Mixtral think/polish |
| c2 | 5201 | P106 | Corrector 7B (optional; not in 3-step pipeline) |
| c2 | 5203 | 1070 | **Idle** (reserved; no PAMP in this flow) |
| T440 | 5001 | P100-0 | Lane A Gemma coder |
| T440 | 5002 | P100-1 | Lane B Qwen14 coder |
| T440 | 5010 | CPU (unified NUMA) | Lane B Qwen3.6 MoE think/polish |

Bring-up: `scripts/t440_dual_lane_layout.sh` (T440), `scripts/cesarops2_dual_lane_layout.sh` (c2).

**Watchdog vs Qwen14:** `mission_service_watchdog` + `gpu_slot_watchdog` can restore the last model on `:5002` (often Qwen3.6 if that was snapshotted). To code with Qwen14 only: `bash scripts/forge_llm_watchdog_off.sh` then `start_qwen14_coder_p100.sh`. See `docs/GPU_SLOT_WATCHDOG.md`.

Lanes may run **in parallel** when c2 RTX + T440 P100s are split; Gemma and Qwen14 use **different P100s**.

Master prompts: `scripts/role_bench/forge_lane_master_prompts.json` (loaded by `run_dual_lane_forge_pipeline.py` when present).

## 3. Prompt tightening (summary)

### Lane A

- **Thinker:** `f64` arrays, discrete equation steps, shape contracts before coder handoff.
- **Coder:** Rust only, flat loops, no nested closures (P100 VRAM).
- **Polisher:** Match nautivecs + OpenMemory; verify Nauticuvs directional criteria.

### Lane B

- **Thinker:** explicit types (`SignalBundle`), `Result<T, EngineError>`, single-flight lock.
- **Coder:** serde/GDAL wrappers; unsafe only behind safe API.
- **Polisher:** OpenMemory heuristics — `Z_max = 4.0`, edge erosion, material fusion weights.

## 4. Hardware (CPU fallback)

T440 unified NUMA node 0: may use **all physical cores** across both Xeon sockets (`-t <total_physical>`, `--numa distribute` or `isolate` per BIOS). Do not stack Mixtral + Qwen14 on the same 1070.

```bash
./llama-server -m <model>.gguf -c 32768 --ngl 0 --numa distribute -t <TOTAL_PHYSICAL_CORES> --host 0.0.0.0 --port 8080
```

## 5. nautivecs math blueprints

Implemented in `nautivecs/src/blueprints/satellite_physics.rs`:

- Secchi depth inversion (Lee-style `K_d` → `Z_sd`)
- C-band Bragg wavelength + slick damping ratio
- Landsat B10 Planck brightness temperature

## 6. OpenMemory operational records

`data/forge/openmemory_straits_records.jsonl` — calibration anchors (Cedarville/Burns), optical Z bug, temporal persistence gate, fusion material weights.

## 7. Straits sensor stack (topics)

| Lane | Sensor | Topic |
|------|--------|--------|
| 1–2 | Sentinel-2 B02/B03 | Blue-green clarity, glint roughness; **accel:** Coral TPU `/infer` + T440 Movidius `jitter-rs` (`docs/GLINT_ACCEL_STACK.md`, `run_straits_glint_accel.sh`) |
| 4 | Sentinel-2 multi-date | Temporal persistence + coregistration |
| 5 | Sentinel-1 RTC | SAR backscatter / Bragg slicks |
| 6 | Landsat ST_B10 | Thermal cold/heat sink |
| 7–8 | SWOT / ICESat-2 ATL13 | Corroboration only |
| — | fusion.rs | Material-weighted composite |

See `docs/FLEET_TOOL_SPECS.md` for implementation shards.
