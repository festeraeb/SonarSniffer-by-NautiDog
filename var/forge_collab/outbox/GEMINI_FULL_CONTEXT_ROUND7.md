# CESAROPS — Full context for Gemini (Round 7)

**You are a physics/review partner, not the implementor.** Cursor ships Rust in-repo; we run `sat-run` and `cesarops-aeromagnetic-worker` on real data. **Push back** when a suggestion is over-scoped, contradicts shipped code, or ignores Great Lakes constraints. Put rejections in `code_patches_rejected` with one-line reasons.

**Repo:** `/data/codebase/repos/wreckhunter2000-1`  
**Primary GT:** Michigan Preserves dive-verified wrecks — `scripts/known_wrecks_straits.json`, mission `gt_min_confidence: 1.0` (17 in Straits bbox).  
**Operator priorities:** (1) finish **satellite detection path** on preserve GT, (2) **aeromag** for steel — especially **off-axis** and **sub-threshold** hits, (3) **well vs abandoned well vs wreck** discrimination (only aeromag tuning Thomas wants), (4) **NauticUVs curvelet science** where it actually helps, (5) **you tell us tools we forgot** per pipeline (§2 inventory + §7 gap ask).

**Forge:** Off the hot path. Twin **P100s** for satellite **math** (FFT, LOO stacks), not LLM inference. Watchdog can be off for Qwen14 on `:5002` when coding.

**Deep references (if you need more):**
- `docs/TOOL_STACK_GEMINI_BRIEF.md` — all tools, formulas
- `docs/DETECTION_PATH_ROADMAP.md` — checklist
- `docs/SATELLITE_P100_OFFLOAD.md` — P100 plan
- `docs/GLINT_ACCEL_STACK.md` — TPU/Movidius
- `docs/SATELLITE_AEROMAG_PUNCHLIST.md` — gaps G1–G10, A1–A4
- `nauticuvs/src/lib.rs` — FDCT (Candès et al. 2006) docs in crate

**Prior rounds:** Rounds 1–3 (bag survey / glint LOO) largely complete. Round 5 tool-stack + Round 6 detection-path pastes may still be unanswered — **this round supersedes narrow asks** with full picture.

---

## 1. Architecture (do not collapse lanes)

```text
LANE A — Satellite (cesarops-satellite / sat-run)
  weather + lake_levels → scene pick → POC / temporal / SAR / bathy → validate_gt → report
  Targets: steel + wood @ ~100 ft; wood = optical/glint/temporal; steel += SAR/thermal/mag

LANE B — Aeromagnetic (SEPARATE binary: cesarops-aeromagnetic-worker)
  Only when TMI/RMI **grids exist** for AOI. Steel/iron hulls. NOT in straits_local_run.json today.

LANE C — Verification / accel
  BAG sonar (optional), TPU :8092 + Movidius :8180 glint accel, Forge (sunset)

SIDEcar — drift.rs (offset physics, NOT a detector)
```

**Thomas rule:** Do not merge aeromag into Straits optical mission for Burns-class wood wrecks (no ferrous mass).

---

## 2. Tool inventory — what is IN each pipeline (read this first)

**Critical ask:** We listed every tool we believe we have. **You must respond with tools we are missing** — sensors, physics, post-processing, or corroboration steps that belong in satellite, aeromag, or verification lanes for Great Lakes steel/wood wreck search @ ~100 ft. Use JSON field `missing_tools_by_pipeline` (see §7). Do not only tune existing knobs; name **absent** capabilities.

Legend: **IN** = implemented and callable · **STAGE** = `sat-run` mission stage · **SPEC** = designed but not on Straits hot path · **BACKUP** = Python in laptop backup, not repo live path · **NOT IN MISSION** = exists in crate but omitted from `straits_local_run.json`

### Pipeline A — Satellite (`cesarops-satellite` / `sat-run`)

**Entrypoint:** `sat-run --spec <mission.json>` · **Straits mission stages today:**  
`target_known` → `poc_aoi` → `bathy_map` → `sar_local` → `temporal_stack` → `validate_gt` → `report`

| sat-run stage | Tool / module | What it does | In Straits mission? |
|---------------|---------------|--------------|---------------------|
| `download` | STAC + chip fetch (`mission.rs`) | Pull/catalog Sentinel-2 (and related) by bbox/days | **NOT IN MISSION** (uses local NFS tiles via `use_local_scenes`) |
| `target_known` | GT loader + targeting CSV | Anchor search to `known_wrecks_straits.json` | **IN** |
| `poc_aoi` | `poc.rs` — see concepts below | Per-scene optical detectors on AOI chips | **IN** |
| `bathy_map` | `bathymetry_map.rs` | Multi-pass SDB Stumpf/Lyzenga + NDTI weights + relief | **IN** |
| `sar_local` | `sar.rs` | Local RTC window extract, ±3σ clusters, DBSCAN persist | **IN** (blocked GDAL) |
| `temporal_stack` | `temporal.rs` + `phase_corr.rs` | Clarity LOO + glint LOO; phase-corr align; persistence maps | **IN** |
| `bag_local` | `stage_bag_local` → external BAG scan | Hook for multibeam `.bag` on disk | **NOT IN MISSION** |
| `validate_gt` | GT validator | Hit/miss vs preserve wrecks; concept scores | **IN** |
| `report` | `fusion.rs` + `mission_report.json` | Material-weighted fuse; ranked candidates | **IN** |

**POC concepts inside `poc_aoi` (all in `poc.rs`):**

| Concept function | Bands / math | Steel ~100 ft | Wood ~100 ft |
|------------------|--------------|---------------|--------------|
| `concept_blue_green_clarity` | B02/B03 ln ratio, local z (cap 4) | Secondary | **Primary** |
| `concept_glint_roughness` | B02/B03 Sobel on variance | Bragg helper | **Primary** |
| `concept_zebra_clarity` | B02/B04 Secchi residual 50 px | Secondary | Mussel/clarity |
| `concept_sediment_plume` | NDTI B04/B03, Δ vs baseline | Post-storm | Runoff plume |
| `concept_shadow_roughness` | B08 NIR Sobel | Deep steel weak | **Avoid deep wood** |

**Pre-stage / parallel tools (same crate, not always a stage):**

| Tool | Module | Role | In Straits path? |
|------|--------|------|------------------|
| Scene/day classifier | `env_conditions.rs` | Calm/storm/post-storm/spring; suitability score | **SPEC** (partial in download) |
| Lake level year rank | `lake_levels.rs` | `ScanIntent` year pick | **SPEC** |
| Drift back-projection | `drift.rs` | Wind/current offset model | **SIDEcar** — not a detector |
| Mag chip derivations | `magnetic.rs` | NSS, VDR, Tilt 3-channel | **SPEC** — hints only, not aeromag pipeline |
| Curvelet rescore | knobs `use_curvelet_rescore` | Would call NauticUVs FDCT on chips | **NOT WIRED** |
| Phase correlation | `phase_corr.rs` | Used inside temporal | **IN** (via temporal) |

**Satellite — separate HTTP accel (outside `sat-run`, fused post-hoc):**

| Tool | Endpoint / script | Role |
|------|-------------------|------|
| Coral TPU infer | `:8092` | Glint/hydrocarbon chip class |
| Movidius jitter | T440 `:8180` | Thermal time-series vote |
| Fuse script | `scripts/role_bench/fuse_glint_accel.py` | Merges accel + `glint_persistence_map.json` |
| Runner | `scripts/role_bench/run_straits_glint_accel.sh` | Orchestrates accel pass |

**Satellite — fleet / spec tools NOT in `sat-run` today:**

| Tool | Status | Notes |
|------|--------|-------|
| Landsat thermal ST_B10 | **SPEC** (FLEET TOOL 2) | Cold/hot anomaly by thermal regime — steel |
| SWOT / ICESat-2 | **SPEC** (FLEET 3–4) | 2 km / along-track depression — fusion bonus only |
| `nauticuvs_satellite_scan.py` | **BACKUP** | Real FDCT on Sentinel — punchlist D1 |
| `detection_scan` / `:5580` worker | **SPEC** | Tile CNN scan — orchestrator gap G3 |
| `overlay_grid` sub-pixel stamps | **BACKUP** | temporal v2 alignment |

---

### Pipeline B — Aeromagnetic (`cesarops-aeromagnetic-worker`)

**Entrypoint:** `cesarops-aeromagnetic-worker detect --grid --meta --levels [--wells ogsr.csv]`  
**NOT invoked by `straits_local_run.json`.** Needs pre-built TMI/RMI grid + GeoJSON meta for AOI.

**Single `detect` pipeline — tools in strict order:**

| Step | Rust module | Tool name | Function |
|------|-------------|-----------|----------|
| 1 | `adaptive.rs` | Adaptive background scan | Rolling local z + edge z; connected components; `top_n` seeds |
| 2 | `gpu.rs` + `dipole_shader.wgsl` | GPU dipole lobe scan | Fast ± lobe score per pixel |
| 3 | `dipole_analysis.rs` | CPU dipole discriminator | Annulus bg, 15% lobe threshold, flip distance, PCA elongation, `score_manmade` 0–100 |
| 4 | `curvelet.rs` | NauticUVs FDCT window | `energy_ratio` detail/coarse; LoG fallback |
| 5 | `pipeline.rs` | Score fusion | adaptive + curvelet + manmade weight |
| 6 | `discriminator.rs` | Well/wreck cross-ref | OGSr wells, known wrecks, Loran-C warp |
| 7 | `well_loader.rs` | OGSr CSV ingest | `CUR_STATUS`, township, Lake Erie bbox filter |
| 8 | `scoring.rs` | Basin scoring | Western/central/eastern penalties & bonuses |
| 9 | `scoring.rs` | Disposition FP filter | Raised/scrapped wreck tags |
| 10 | `pipeline.rs` | Merge NMS | 2500 m dedupe |
| 11 | `known_data.rs` | Embedded GT wrecks | NDA + ShipwreckWorld + **Straits preserve** list |

**Other aeromag subcommands / modules (not full detect path):**

| Tool | Module | Role |
|------|--------|------|
| `dipole` (legacy) | GPU only | Dipole-only scan without adaptive/curvelet |
| `continuation` | `continuation.rs` | Upward/downward FFT continuation; satellite proof @ 400 km |
| `datum` | `datum.rs` | Grid/datum helpers |
| `geo` | `geo.rs` | Pixel ↔ lon/lat |
| `describe` | CLI | Machine-readable catalog for orchestrators |
| `test-all` | CLI | Demo grid self-test |

**Python orchestration (BACKUP / live_reference — may not be in repo root):**

| Tool | File | Role |
|------|------|------|
| Erie central orchestrator | `erie_central_aeromag_orchestrator.py` | Knobs, merge, manmade gate |
| Wellhead discriminator | `erie_wellhead_discriminator.py` | Wells + wrecks + Loran |
| Geo filter + off-axis bonus | `geo_filter_candidates.py` | **+10° off NE-SW strike** — **not in Rust** |
| Flight-line physics v2 | `flight_line_physics_v2.py` | Along-track detrend / unmask — **not in Rust** |
| Mag rust bridge | `mag_rust_detect.py` | Python → worker binary |
| NauticUVs mag curvelet | `nauticuvs_mag_curvelet.py` | Reference for `curvelet.rs` |
| XGBoost off-axis trainer | `train_lake_erie_offaxis.py` | **SPEC** — not in worker |
| Erie synthetic dipole | `erie_synthetic_dipole.py` | Training synthetics |

**Support (cross-domain, satellite crate):** `cesarops-satellite/src/magnetic.rs` — NSS/VDR/Tilt chips for **satellite-side** magnetic hints only.

---

### Pipeline C — Verification & corroboration (independent of sat-run)

| Tool | Crate / script | Pipeline stages | Role |
|------|----------------|-----------------|------|
| BAG multibeam scan | `cesarops-bag-scan` | A Read → B Geo → C Anomaly → D Redaction → E Orientation → F Dedup → G Report | Seafloor height-above-floor; redaction unmask |
| BAG in sat-run | `stage_bag_local` | Optional hook | **Off Straits mission** |
| Glint accel | scripts + TPU/Movidius | Post `temporal_stack` | Corroborate glint persistence |
| Forge dual-lane LLM | `:5001` Gemma, `:5002` Qwen14, c2 Mixtral | **Sunset on hot path** | Spec/review only |
| n8n / mission watchdog | JSON workflows | Ops | Dispatch; path gaps in punchlist G6 |

**BAG internal tools (`cesarops-bag-scan`):** `bag_io`, `anomaly`, `redaction_unmask`, `orientation`, `dedup`, `grid`, `unmask` export GeoTIFFs.

---

### Sidecars & math libraries (not standalone pipelines)

| Name | Package | Used by |
|------|---------|---------|
| NauticUVs FDCT | `nauticuvs/` | Aeromag `curvelet.rs`; **not** sat-run hot path |
| nautivecs blueprints | `nautivecs/` | Secchi, Bragg, Planck — Forge/RAG truth, not runtime detector |
| OpenMemory | `:8765` | Calibration records (gates, fusion weights) |

---

### Cross-pipeline fusion rule (`fusion.rs`)

**Detector families counted toward “triple lock” (operator standard ≥3 of 5):**  
optical POC, temporal persistence, SAR, bathy relief, thermal (spec), aeromag (separate), BAG (optional), SWOT/ICESat (spec).

Material tags: **steel** → weight SAR + thermal + aeromag; **wood** → optical + temporal + glint; down-rank shadow_roughness on deep wood.

---

## 3. Satellite lane — what is shipped vs broken vs needs work

### Mission (Straits local)

`data/missions/straits_local_run.json`  
Stages: `target_known`, `poc_aoi`, `bathy_map`, `sar_local`, `temporal_stack`, `validate_gt`, `report`  
Runner: `bash scripts/role_bench/run_detection_path.sh` → `detection_path_report.json`

### Shipped (Rust, in `cesarops-satellite`)

| Module | Role | Notes |
|--------|------|-------|
| `env_conditions.rs` + `lake_levels.rs` | Scene/day pick | `docs/SCENE_SELECTION_PHYSICS.md` |
| `poc.rs` | blue_green_clarity, glint_roughness, zebra, plume | Z cap 4.0, edge margin |
| `temporal.rs` + `phase_corr.rs` | LOO persistence, align, reject shift >64px | GT gate 0.08 @300m else 0.3 |
| `bathymetry_map.rs` | Stumpf/Lyzenga multi-pass, NDTI weights | ~20–30m depth trust in turbid Straits |
| `sar.rs` | Local RTC extract + DBSCAN | **BLOCKED: GDALOpenEx NULL** on RTC GeoTIFF |
| `fusion.rs` | Material weights steel vs wood | Triple-lock concept |
| `drift.rs` | Current/wind back-projection | Sidecar only |
| `magnetic.rs` | NSS/VDR/Tilt chips | Cross-domain hints, not full aeromag pipeline |

### Measured / learnings (Cursor, not theory)

| Experiment | Result |
|------------|--------|
| Temporal LOO @ Cedarville/Burns | persist ~0.06–0.08; **one GT hit ~0.118** (structurally valid) |
| Glint roughness POC | **417 candidates**; nearest peaks **11–15 km** from GT — needs NMS/gate tuning |
| SAR local | Implemented; **GDAL open fails** on our RTC path |
| Glint accel smoke | TPU + Movidius agree on green chip; `fuse_glint_accel.py` ready |
| Phase-corr | Shipped in Rust; early run 0 temporal cands before gate fixes |

### Satellite — prioritized work (Cursor owns)

1. **Fix SAR GDAL RTC open** (`sar.rs` + path to `data/straits_sar/sar`)
2. **Tune temporal/glint gates** from metrics + your review (not blind threshold cuts)
3. **Calibrate bathy** m0/m1 on shallow preserve control points; wire relief into fusion
4. **P100 phase 1** — scene-parallel POC / FFT per `SATELLITE_P100_OFFLOAD.md` (after you rank modules)
5. **Optional:** `use_curvelet_rescore` on satellite — **NOT wired live** (see NauticUVs section)

### Known punchlist bugs (Python orchestrator — may affect dry runs)

- G1: `gt_wreck_names` filter inverted in old `sat_mission_orchestrator.py`
- G2: dry_run `validate_gt` returned `no_data`
- G3: detection_scan empty `image_b64`
- G8: temporal_stack v1 catalogs STAC but doesn't download B03/B08 chips for full stack

**Push back if you suggest:** rewriting sat-run in Python; blocking Straits on Erie aeromag grids; using shadow_roughness for deep wood.

---

## 4. Aeromagnetic lane — full pipeline (Thomas focus)

**Binary:** `cargo run -p cesarops-aeromagnetic-worker -- detect --grid … --meta … --levels … [--wells ogsr.csv]`  
**Python reference (laptop backup, not live path):** `backup/deploy/tools/pipelines/mag/*`, `live_reference/mag__erie_wellhead_discriminator.py`

### Stage order (actual Rust `pipeline.rs`)

1. **Adaptive** — local z on \|field\|; mask `z_thresh` + `edge_z_thresh`; components; **top_n=500** default.
2. **GPU dipole** (WGSL) — fast lobe score, inner/outer yards from knobs.
3. **CPU dipole** (`dipole_analysis.rs`) — annulus bg; ± lobes @ **15% peak_abs**; flip walk; gradient contrast; PCA **`elongation_azimuth_deg`** (0–180°); **`score_manmade` 0–100**.
4. **Curvelet** (`curvelet.rs`) — **real `nauticuvs::curvelet_forward`** (FDCT f64); fallback LoG proxy on error.
5. **Fuse** — `combined = base*(1-w) + (manmade/100*10)*w`.
6. **Discriminator** — OGSr wells + known wrecks; **Loran-C warp** before distances.
7. **Basin scoring** (`scoring.rs`) — well penalties; wreck bonuses; **central basin ×0.7 if monopolar** (well-like).
8. **Merge** — 2500 m NMS.

### Off-axis (what Thomas hunts)

- **Physics intent:** Ferrous wrecks often show dipole axes **misaligned** with regional Precambrian **NE–SW strike (~45°)**. Geology aligns with strike; compact cultural targets differ.
- **Computed today:** `elongation_azimuth_deg` from PCA on significant inner pixels (`dipole_analysis.rs`).
- **Scored in Python backup only:** `geo_filter_candidates.py` — +10 if >60° off strike, −8 if <20° aligned. **Not ported to Rust** — do not claim off-axis ranking is live in worker.
- **ML backup (not in worker):** `train_lake_erie_offaxis.py` — `axis_offset_deg`, flight-line distance; XGBoost per basin. **Defer** unless we have labeled CSV.

### Sub-threshold / weak hits (intentional)

| Knob | Default | Low-recall test (`n8n/test_fixtures/mag_levels.json`) |
|------|---------|------------------------------------------------------|
| `z_thresh` | 0.38 | 0.15 |
| `min_dipole_manmade_score` | **20** (keeps **AMBIGUOUS**) | **0** |
| `require_dipolar_pull` | true | **false** |
| `dipole_min_score` | 0.35 | 0.0 |

Verdict bands: ≥60 strong, ≥40 moderate, **≥20 ambiguous**, else geological. Thomas wants **review queue**, not auto-drop of weak dipoles.

### Well vs abandoned well vs wreck (only tuning scope)

| Cue | Well-like | Wreck-like (steel) |
|-----|-----------|-------------------|
| Polarity | Monopolar common | Dipolar +/− |
| Basin | Central: **×0.7 if !is_dipolar** | Eastern: +10% if dipolar |
| Catalog | OGSr <500–2000m penalties; <2km → `suspected_wellhead_requires_satellite_check` | Known wreck <1–3km bonuses |
| **Abandoned** | `CUR_STATUS` **loaded** (`well_loader.rs`) but **NOT used in scoring** | Abandoned casing can still be magnetic — **do not auto-reject by status alone** |

**Embedded GT:** Colgate #103 wreck; wellheads #63, #85 (`known_data.rs`). Straits preserve wrecks in `straits_wrecks()` for cross-check when Erie/Niagara grids overlap.

### Aeromag — prioritized work

| Priority | Item | Status |
|----------|------|--------|
| P0 | Port **strike-deviation** bonus/penalty into `scoring.rs` (single score path) | **Missing** |
| P0 | **Coarse-grid asymmetric dipole** mitigation (Python +8) — negative lobe in adjacent cell | **Missing** |
| P1 | `status` on wells → **review flag only**, never hard drop | **Missing** |
| P1 | Prove worker on Erie grid vs Python oracle (#103, #63, #85) | **Needs grid + run** |
| P2 | Wire Python `mag_rust_detect.py` / orchestrator (live `pipelines/mag/` not in repo root) | **Partial** |
| P2 | Flight-line physics v2 — only if gridded survey + line metadata available | **Backup only** |

**Push back if you suggest:** status-only well filter; min_manmade ≥50 for scout; merging mag into `straits_local_run.json`; Forge patching lobe thresholds without GT regression.

---

## 5. NauticUVs — science & parity (please answer carefully)

### What NauticUVs is here

- **Crate:** `nauticuvs/` — **f64 FDCT** (Candès–Donho–Ying 2006): parabolic scaling, multi-angle detail coefficients.
- **Operational claim (CESAROPS):** Separates **anisotropic** structure (ship tracks, wreck-aligned edges, flight-line corrugation, internal-wave fronts) from **isotropic** smooth geology/noise better than raw LoG or wavelets.
- **Do not confuse with:** `nautivecs` RAG blueprints (Secchi, Bragg, Planck) — different package.

### Where it is used today

| Consumer | Implementation | Threshold / weight |
|----------|----------------|-------------------|
| **Aeromag worker** | `curvelet.rs` → `nauticuvs::curvelet_forward` | `energy_ratio = detail_energy / coarse_energy`; gate **≥2.0** default; weight **0.45** in fuse; discriminator +5 per unit above 3.5 |
| **Satellite** | Knobs exist (`use_curvelet_rescore`, window 64, 5 scales) | **NOT on sat-run hot path** — POC uses Sobel/LoG proxies |
| **Python backup** | `nauticuvs_mag_curvelet.py`, `nauticuvs_satellite_scan.py` | Import path / wiring unverified live |

**Fallback:** If FDCT fails, aeromag `curvelet.rs` uses multi-scale LoG proxy (`backend: "log-proxy"`).

### Parity gap (important)

- **Mag pipeline:** Real FDCT in Rust worker.
- **Satellite pipeline:** No FDCT rescore in production `sat-run` — **satellite/mag curvelet parity is incomplete**. Punchlist D1: import `nauticuvs_satellite_scan.py` as optional stage.

### Science questions for you (NauticUVs)

1. **Scales:** Worker uses **6 scales** on **128×128** windows at ~25–100 m/px. For aeromag dipole separation ~hundreds–2000 m, which `num_scales` and window px are physically justified? Should scales track **`log2(min(rows,cols))-2`** per crate docs?
2. **energy_ratio:** Is **detail/coarse** the right statistic for “man-made vs geology,” or should we use **directional wedge energy** vs isotropic coarse only (FDCT angle bins)?
3. **Flight lines:** Aeromag grid has along-track corrugation — does FDCT **amplify** flight-line artifacts unless detrended first? Should curvelet run on **vertical derivative** or **along-track residual** grid (see backup `flight_line_physics_v2.py`)?
4. **Satellite:** For glint/plume **anisotropic** surface patterns, is FDCT on **B02 variance chips** preferable to Sobel roughness — and at what resolution (64 vs 128 vs 256)?
5. **Threshold 2.0 / 3.5:** Empirical or placeholder? Propose basin-specific thresholds (Erie central noisy vs eastern quiet).
6. **f32 vs f64:** Satellite chips often f32; worker uses f32 window → FDCT internal f64. Any numerical concern at 12-bit Sentinel dynamic range?

**We will not** downgrade NauticUVs to f32 “for speed” without physics argument (OpenMemory / handoff rule).

---

## 6. Master “what’s missing” table (Cursor’s list — you must extend)

| Area | Missing / blocked |
|------|-------------------|
| Satellite SAR | GDAL RTC open |
| Satellite curvelet | FDCT rescore stage not wired |
| Satellite temporal v2 | Per-scene B03/B08 download + stack |
| Satellite glint | NMS / distance-to-GT tuning |
| Satellite bathy | m0/m1 calibration; fusion hook |
| Satellite thermal | Landsat ST_B10 stage partial spec |
| Aeromag off-axis | Strike bonus only in Python backup |
| Aeromag wells | `status` not in scoring; abandoned = same morphology |
| Aeromag ops | Live orchestrator path; grid ingest for Straits |
| Collab | Your structured replies rounds 5–6–7 |
| P100 | Scene-shard wrapper not started |
| BAG | Optional; not blocking sat path |

---

## 7. What we need from Gemini this round

Answer in **JSON** (below). Be specific — numeric knobs, ordered lists, **missing tools**, and **defer** list.

### 0. Gap discovery (required — do this first)

Review §2 tool inventory. For **each** pipeline (satellite, aeromag, verification, cross-pipeline fusion), list:

- **`missing_tools`**: sensors, algorithms, or processing steps we should add (e.g. ice mask, chlorophyll, AIS clutter, magnetometer diurnal correction, sub-bottom, sidescan, gravity, EM, lake ice, shoreline buffer, …).
- **`misplaced_tools`**: tools in wrong pipeline (should move or drop).
- **`redundant_tools`**: two tools doing the same job — which to keep?
- **`tools_we_listed_but_you_dispute`**: if you think something in §2 is not a real tool or is misdescribed, say so.

We already know we lack: SAR GDAL fix, satellite FDCT, thermal Landsat stage, flight-line detrend in Rust, strike bonus in Rust, full temporal chip download. **Go beyond that.**

### A. Satellite

1. Tuning order for preserve-GT hits: temporal gate, glint NMS, SAR, fusion, bathy — **with rationale**.
2. Steel vs wood @ ~100 ft: rank tools (align with brief §2–7).
3. P100 offload order: phase_corr FFT, per-scene POC, temporal LOO, bathy fuse — risks on 16 GB Pascal?

### B. Aeromag (Thomas)

4. Where to wire off-axis: `scoring.rs` vs post-CSV geo_filter — avoid double-count with `score_manmade`.
5. Monopolar penalty in central basin: keep ×0.7, soften, or coarse-only mitigation?
6. One JSON **review_queue** knob set vs **shortlist** knob set (numeric).
7. Well vs abandoned: recommend **features** (not ML first) — dipole, extent, distance, status as flag only.

### C. NauticUVs

8. Answers to science questions §5 (numbered).
9. Should satellite adopt FDCT before or after aeromag strike wiring?

### D. Meta

10. `defer` — what **not** to build this month.
11. `pushback_on_cursor` — where **we** may be wrong.
12. `code_patches_rejected` — ideas you refuse to endorse.

---

## Reply JSON schema

```json
{
  "round_id": "cesarops_full_context",
  "round": 7,
  "from": "gemini",

  "missing_tools_by_pipeline": {
    "satellite": [
      { "tool": "name", "why_needed": "", "target_class": "steel|wood|both", "priority": "high|med|low" }
    ],
    "aeromag": [],
    "verification": [],
    "cross_pipeline_fusion": []
  },
  "misplaced_tools": [],
  "redundant_tools": [],
  "tools_we_listed_but_you_dispute": [],

  "satellite": {
    "detection_priority": [],
    "tool_rankings": {
      "steel_100ft": [],
      "wood_100ft": []
    },
    "p100_offload_order": [],
    "p100_risks": ""
  },

  "aeromag": {
    "strike_wiring": "scoring_rs|geo_filter|defer",
    "monopolar_penalty": "keep|soften|coarse_mitigation",
    "review_queue_knobs": {
      "z_thresh": 0.0,
      "edge_z_thresh": 0.0,
      "min_dipole_manmade_score": 0.0,
      "require_dipolar_pull": false,
      "dipole_min_score": 0.0
    },
    "shortlist_knobs": {},
    "well_vs_wreck_features": [],
    "abandoned_well_policy": ""
  },

  "nauticuvs": {
    "scales_and_window": "",
    "energy_metric": "detail_over_coarse|directional_wedge|other",
    "flight_line_handling": "",
    "satellite_fdct_recommendation": "",
    "thresholds_by_basin": {}
  },

  "defer": [],
  "pushback_on_cursor": [],
  "code_patches_rejected": [],
  "collab_complete": false,
  "gemini_confidence": 0
}
```

Save reply to: `var/forge_collab/inbox/gemini_ack_round7.json` (or paste in chat; Cursor runs `scripts/forge_collab/merge_gemini_ack.py` if configured).

---

## Cursor implementation policy (for your awareness)

- We implement Rust; we question your code if it fights shipped modules.
- We run `prove_tools_preserve_gt.sh` and `run_detection_path.sh` for evidence.
- We will **not** wait for Forge to compile-test aeromag or satellite changes.
