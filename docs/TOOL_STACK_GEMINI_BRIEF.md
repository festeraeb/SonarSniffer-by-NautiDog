# CESAROPS tool stack — Gemini review brief

**Ask:** For each tool below, review the math/physics, say which is strongest for steel vs wood vs ~100 ft depth, what is missing, and suggest tuning order. Ground truth for proving satellite tools: **dive-verified Michigan Preserves** wrecks in `scripts/known_wrecks_straits.json` (`gt_min_confidence: 1.0`).

---

## Pipeline architecture (three lanes)

```text
┌─────────────────────────────────────────────────────────────────────────┐
│ LANE A — Satellite (cesarops-satellite / sat-run)                       │
│  weather + lake_levels → scene pick → POC/temporal/SAR → validate_gt   │
└─────────────────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────────────────┐
│ LANE B — Aeromagnetic (SEPARATE — only when mag grids exist)            │
│  cesarops-aeromagnetic-worker + pipelines/mag/* orchestrator            │
│  → steel / iron hulls; NOT fused into sat-run by default                │
└─────────────────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────────────────┐
│ LANE C — Independent verification                                       │
│  cesarops-bag-scan (NOAA BAG) · accel TPU/Movidius · Forge LLMs (sunset) │
└─────────────────────────────────────────────────────────────────────────┘
```

**Drift** (`cesarops-satellite/src/drift.rs`) is a **physics sidecar**: it does not detect wrecks. It back-projects surface observations along wind/current to explain why a plume or glint centroid may be offset from the seabed source when the temporal stack spans weeks of current.

---

## 0. Weather + scene selection (runs before detectors)

**Modules:** `env_conditions.rs`, `lake_levels.rs`, `mission.rs` download stage  
**Doc:** `docs/SCENE_SELECTION_PHYSICS.md`

### What it does

Picks **which satellite days and years** enter the stack — as important as the detectors.

| Input | Source | Role |
|-------|--------|------|
| Daily wind, precip, cloud | Open-Meteo archive API | `classify_day` → Calm / Storm / PostStorm(n) / SpringRunoff / Transitional |
| Great Lakes water level | NOAA GLERL year ranking | `ScanIntent` → which years to pull (not always “lowest water”) |
| Wreck depth + month | GT / preserve DB | `thermal_regime(depth_ft, month)` → AlwaysCold vs SunCycling |
| Cloud cap | Mission knob `max_cloud` | STAC sort; lowest cloud first |
| 20-day stack window | `stack_window_days` | Contiguous low-cloud window so temporal alignment + drift back-projection stay valid |

### Math / rules (summary)

- **Photic depth** varies by month (~90 ft winter → ~220 ft summer). Wrecks deeper than that → **AlwaysCold** (persistent thermal sink); shallower → **SunCycling** (image at thermal extreme).
- **Spring runoff** (Mar–May, melt/rain, not storm): best **sediment plume** window; also raises current → more surface ripple over structure.
- **Post-storm** (1–3 days after): suspended sediment over wreck after surface calms.
- **Clarity / zebra** intents **penalize** turbid runoff days.
- Scene score ≈ cloud rank × `condition_suitability(day, ScanIntent)`.

### Strongest for

- Every optical/temporal pass — wrong days add false glint and break LOO persistence.
- **Not** a wreck detector; gates data quality.

### Missing / tune

- Wire Open-Meteo fetch into automated mission download (partially in download stage).
- Per-wreck `ScanIntent` in mission JSON (Cedarville → low water + steel thermal; wood → zebra_clarity + glint).
- Explicit “pick satellite days” report JSON for operator review.

---

## 1. Satellite-derived bathymetry (SDB) — **BathymetryMapper / `bathymetry_map.rs`**

**Recovered name:** `recovery/laptopdump/cesarops_core/src/cesarops/bathymetry_mapper.py` (Phase 11.2)  
**Rust:** `cesarops-satellite/src/bathymetry_map.rs` — stage `bathy_map`  
**Operator intent:** Multi-pass mapping of lake bottom / shoal relief around **~100 ft (30 m)** targets; turbidity gates which passes contribute.

### Math

| Step | Formula / method |
|------|------------------|
| Secchi proxy | `3.9·√(B02/B04) + 0.55` (m) |
| Depth limit per pass | `min(depth, 2.5 × Secchi)` — Straits Secchi ~8–12 m → **~20–30 m** trustworthy bottom |
| Stumpf | `Z = m0 − m1·ln(Rrs_blue)` (defaults m0=0, m1=8.5 — **calibrate on shallow preserve wrecks**) |
| Lyzenga proxy | `6 + 12·(ln B02 − ln B03)` |
| Turbidity weight | `exp(−3·max(0, NDTI−0.15))` on NDTI = (B04−B03)/(B04+B03) |
| Multi-pass fuse | Weighted mean depth per pixel across scenes |
| Relief | Gradient magnitude on fused depth (drop-offs / shoal flanks) |

### Strongest for

- Shoal mapping around deep wrecks (map flanks &lt;30 m even when hull is at 34 m).
- Column turbidity change (plume) when bottom reflectance is invisible.
- **Weak** for direct hull imaging at 100 ft+ in turbid Straits — physics cap is ~2–3× Secchi.

### Missing / tune

- Calibrate m0/m1 against Cedarville / shallow preserve control points.
- Export fused GeoTIFF + contour JSON (Python had GeoJSON/KML/NetCDF).
- Feed relief peaks into `fusion.rs` as `concept_scores["bathy_relief"]`.

---

## 2. Optical POC concepts (`poc.rs`)

| Concept | Bands | Math | Steel | Wood ~100 ft |
|---------|-------|------|-------|----------------|
| `blue_green_clarity` | B02/B03 | `ln(B02)/ln(B03)`, local z, cap \|z\|≤4 | Secondary | **Primary** (column disturbance) |
| `concept_glint_roughness` | B02/B03 | Sobel on local variance map | Surface Bragg helper | **Primary** (current roughness) |
| `zebra_clarity` | B02/B04 | Secchi residual 50 px window | Secondary | Mussel/clarity lane |
| `sediment_plume` | B04/B03 NDTI | Absolute or Δ-NDTI vs clear baseline | Post-storm anchor | Runoff plume |
| `shadow_roughness` | B08 NIR | Sobel — **does not penetrate water** | Deep steel poor | **Avoid for deep wood** |

**Local runner:** `run_poc_local` on NFS Sentinel-2 tiles.

---

## 3. Temporal stack (`temporal.rs`)

| Piece | Math |
|-------|------|
| Clarity LOO | Per-scene blue-green clarity → 50 px `baseline_residual` → phase-corr align (`phase_corr.rs`, reject shift &gt;64 px) → leave-one-out persistence |
| Glint LOO | `glint_variance_map` + same baseline/align/LOO |
| GT gate | Dynamic: 0.08 within 300 m of preserve coord, else 0.3 |

**Strongest for:** Persistent optical signature (wood); steel when combined with thermal/SAR.

**Drift link:** Gross misalignment rejected; fine offsets explained by `drift.rs` when stack is ~20-day window.

---

## 4. SAR (`sar.rs`) — stage `sar_local`

| Piece | Math |
|-------|------|
| Extract | Local mean/std windows; ±3σ bright/dark clusters in RTC GeoTIFF |
| Persist | DBSCAN on (lat,lon); orbit groups ascending/descending |

**Strongest for:** **Steel** (Cedarville, Eber Ward) — double-bounce + surface slick modulation.  
**Weaker for:** Wood at depth (no metallic return).

---

## 5. Thermal (FLEET TOOL 2 — spec, partial)

Landsat ST_B10: `DN·0.00341802+149` → K; local z; cold **and** hot anomalies by regime.

**Strongest for:** **Steel** deep cold sink (Cedarville). Down-weight for wood per fusion rules.

---

## 6. SWOT / ICESat-2 (FLEET TOOLS 3–4 — corroboration only)

2 km / along-track surface height depression — fusion bonus, not standalone.

---

## 7. Glint accel (TPU + Movidius) — **separate HTTP tools**

| Endpoint | Math / role |
|----------|-------------|
| Coral `:8092/infer` | Chip PNG scout (glint/hydrocarbon class) |
| T440 `:8180/jitter` | Thermal time-series signature; Movidius `myriad_fp16` vote |

**Script:** `run_straits_glint_accel.sh` · **Fuse:** `fuse_glint_accel.py` with `glint_persistence_map.json`  
**Doc:** `docs/GLINT_ACCEL_STACK.md`

---

## 8. Aeromagnetic pipeline — **SEPARATE from sat-run**

**Crate:** `cesarops-aeromagnetic-worker`  
**Orchestrator:** `pipelines/mag/erie_central_aeromag_orchestrator.py`  
**Support:** `magnetic.rs` (NSS/VDR/Tilt 3-channel chips for cross-domain hints)

### When to use

- **Only when aeromag survey grids exist** for the AOI (Erie, Niagara, flight-line TMI/RMI).
- Targets **ferrous / steel** hulls: dipole discrimination, adaptive background z-score, curvelet enhancement (nauticuvs).

### Math (worker)

| Module | Method |
|--------|--------|
| `adaptive.rs` | Rolling background; anomaly z-score along flight lines |
| `dipole_analysis.rs` | Dipole fit / discrimination vs geologic noise |
| `curvelet.rs` | Directional energy (ship track alignment) |
| `discriminator.rs` | Score candidates vs known wreck priors |

### Strongest for

- **Steel wrecks** (Cedarville-class).  
- **Not run** for wood-only Mackinac wood targets (Burns hypothesis) — no magnetic mass.

### Missing / tune

- Wire orchestrator to Forge/n8n (punchlist A1).
- Straits preserve GT in `known_data.rs` — validate dipole hits on Cedarville/Eber Ward when grid coverage exists.
- Do **not** block satellite bench on aeromag availability.

---

## 9. Drift model (`drift.rs`) — **SEPARATE physics lane**

**Purpose:** Explain **where** a surface signal should be relative to the wreck on the bottom when currents/wind differ across passes.

| Piece | Math |
|-------|------|
| `basic_drift` | `u = current_u + windage·wind_u` (default windage 3%) |
| `forward_drift_run` / `backward_drift_run` | Storm phases with constant wind/current per phase; haversine steps |
| `analog_storm_score` | Match buoy/storm analogs (ports `buoy_analog.py`) |

**Used with:** Temporal stack (plume advection up to ~200 m offset), scene selection (20-day window keeps drift bounded).

**Not a detector** — does not emit wreck candidates.

---

## 10. BAG sonar (`cesarops-bag-scan`) — **independent verification**

NOAA multibeam `.bag` HDF5: height-above-floor anomalies, redaction unmask.  
**Optional** — prove against preserve coords when files on disk; not on `sat-run` critical path.

---

## 11. Fusion (`fusion.rs`)

Material-weighted `SignalBundle`: steel → SAR+thermal; wood → optical+temporal_persistence.  
Triple-lock: ≥3 of 5 detector families (operator standard).

---

## Prove-out command (preserve GT)

```bash
bash scripts/role_bench/prove_tools_preserve_gt.sh
```

17 dive-verified wrecks in Straits AOI bbox (Cedarville, Eber Ward, M. Stalker, …).

---

## Suggested tuning order (Cursor — pending Gemini reply)

1. Weather/scene intent per target class (steel vs wood).
2. `bathy_map` calibrate Stumpf on shallow preserves; turbidity weights.
3. Glint LOO + accel corroboration thresholds @ Burns proxy coords.
4. SAR GDAL RTC open (steel GT).
5. Thermal Landsat untar + persistence (Cedarville).
6. Aeromag worker vs Cedarville **only if** grid loaded.
7. Drift back-projection report for temporal offset diagnostics.
8. BAG optional corroboration.

---

## Gemini reply schema (paste in Google AI thread)

```json
{
  "round_id": "straits_tool_stack_review",
  "from": "gemini",
  "tool_rankings": {
    "steel_100ft": ["...", "..."],
    "wood_100ft": ["...", "..."]
  },
  "missing_math": ["..."],
  "tuning_priority": ["..."],
  "pushback_on_cursor": []
}
```
