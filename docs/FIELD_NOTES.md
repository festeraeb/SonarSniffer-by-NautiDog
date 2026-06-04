# Field Notes — wreck-logic / why the code does what it does

> Mission/origin context lives in `docs/PROJECT_ORIGIN_AND_MISSION.md` — read
> that for the *why of the project*; this file is the *why of the code*.

Version-controlled TRIBAL KNOWLEDGE. Code can be rewritten after a crash; the
*reasoning* is what vanishes. This file exists because we lost the original
system (drive crash) and then lost discoveries again (AI-assisted rebuild that
never wrote the subtle findings back). If you learn WHY something works or
fails, append a dated entry here BEFORE you forget. Keep it terse.

Format:
```
YYYY-MM-DD  <short title>
  Finding: ...
  Cause:   ...
  Fix:     ...
  Evidence:...
```

---

## Detection physics (the load-bearing ideas)

- **Not direct hull imaging.** A wreck chills/disturbs the water column; the
  disturbance rises toward the thermocline and is read OPTICALLY in blue-green
  at ~180 ft apparent depth. The multi-year stack captures the plume ADVECTED
  by the current, so a wreck "appears" to move a few hundred ft between dates.
  Back-project the drift to the true seabed source.
- **Material changes the sensor mix.** Cold/heat-sink (thermal) is STRONG on
  steel (Cedarville), WEAK on wood (Burns). Wood signals via physical
  obstruction → zebra-clarity + current-driven glint modulation + shadow.
  Down-weight thermal for wood; up-weight clarity + glint.
- **Thermal regime = depth vs sunlight, NOT day/night of the same wreck.**
  Deep (> photic depth) = always cold, any pass. Shallow = heats by day / cools
  by night → image at a thermal extreme (afternoon peak or pre-dawn). The photic
  cutoff shifts with season (~130 ft spring → ~220 ft summer), so a 150 ft wreck
  is "always cold" in April but "cycling" in August.
- **SDB caps ~80 ft.** Beyond ~2-3× Secchi the bottom returns no reflectance;
  deep targets need temporal persistence of surface/column signals, not bottom.
- **Bands:** B02 blue + B03 green penetrate; B04 red + B08 NIR are surface-only
  (plume / glint). For a deep non-fuel target, red/NIR can be dropped.
- **Glint / current roughness is SIGNAL, not noise.** Straits current over a
  hull modulates surface roughness, visible blue-green.

## Discrimination (rock vs wreck) — learned the hard way

- **Interior relief alone is NOT a wreck discriminator** — glacial ridge fields
  have interior relief too. (See GROUND_TRUTH_LOG 2026-06-03, E-of-Elva.)
- **The decisive test is long-axis azimuth vs channel bearing.** Parallel +
  repeating + channel-aligned = geology. A single ISOLATED, OFF-axis cap is the
  wreck signature. Caveat: a current-aligned wreck can also lie along-channel,
  so "≥2 similar features sharing the channel axis/spacing" = treat the family
  as geology pending another sensor.
- **Floor-context test (cap on a smooth thalweg) is WEAKER than I thought** — it
  pointed wrong at E-of-Elva. Trust azimuth + parallel-neighbor-count more.
- **Rocks here are different:** near the Elva there's a ~40 ft flowerpot/dolomite
  pillar. A monolith = single convex dome, L:B ~1 at every threshold, one
  maximum. A hull = elongated (L:B ≥ ~2.5), multiple interior maxima, gets MORE
  linear as you raise the threshold.
- **Detrend before measuring.** A 4-8 m BAG tile around a target includes a
  sloping seabed; a percentile threshold over the whole tile measures the tile,
  not the object. Subtract a large-window background, then flood-fill the object
  from its peak. (isolate_object.py)

## Coordinate / grid / warp lore (the stuff that bites silently)

- **GeoTIFF slicing breaks spatial integrity.** Align to the ORIGINAL raster
  BEFORE any coordinate conversion. Reproject as few times as possible.
- **Correction is rotational/trigonometric, not linear.** A linear shift fix
  failed; subpixel SHAPE alignment before coordinate conversion is what worked
  (known-wreck alignment improved).
- **LORAN-C-style warp is non-uniform.** Don't assume a constant offset.
- **Some errors follow a spiral pattern from a centroid.** Watch for it.
- **Phase-corr returns Ok(0,0) for pre-aligned local tiles** — origin-peak fix;
  skip phase-corr entirely for local tiles.

## BAG / redaction lore

- **A BAG mask is its own candidate.** A deliberate hide is evidence; it also
  contributes one signal to the satellite triple-lock.
- **The uncertainty band is MANDATORY in the BAG spec** — the file breaks
  without it; it carries the sounding footprint even when elevation is flattened.
- **FOIA backstory:** redaction LENGTH/order leaked finding info; 2 redactor
  signatures were ML-learned from an all-redacted PDF.
- **NOAA masks geology too** — not every masked zone is a wreck (E-of-Elva was
  unique glacial ridges, side-scan-confirmed).

## Pipeline / build lore

- **CARGO_TARGET_DIR=/data/cargo-target** (builds go to /data not /codebase).
  Commit locally, don't push.
- **Build satellite with** `cargo build --release -p cesarops-satellite --features gdal`
  (gdal is an OPTIONAL feature; default build omits it).
- **gdal_array (python) is built against numpy 1.x** and crashes under numpy
  2.4 — use rasterio for raster reads in scripts, or gdalinfo -stats for stats.
- **Per-component morphology must be windowed**, not full-grid (was the BAG
  infinite-hang bug). restore_preview NN interp subsampled to MAX_SAMPLE=256.
- **Jitter signatures are useful features themselves**, not just noise.
- **Curvelet outputs must be validated across stacks** (curvelet_inverse had an
  all-NaN bug).
- **Detuned 32-bit public nauticuvs is intentional access-control.** The native
  f64 build on the drive is the real one — NEVER reduce its precision.

## Sensor wiring reality (2026-06-03) — see pipeline_defaults_inventory.json

- Wired detection: Sentinel-2, SAR (but on SLC → 0 hits, needs RTC), OPERA DSWx.
- Wired selection: NDBC/GLOS buoy calm-gate.
- Download-only (not feeding detection): ICESat-2 ATL13.
- Stub (neutral 0.5): SWOT, ECOSTRESS.
- Absent: ICESat-2 ATL03 (the raw photon cloud — highest-value addition; ATL13
  has already averaged away the column effects we exploit).
- Separate crates: aeromag, BAG bathymetry (not yet in satellite fusion).

## Next-jump consensus (independent engine + operator agree)

The next capability jump is NOT another curvelet tweak. It's wiring
**ATL03 + real SWOT + real ECOSTRESS** into the existing candidate-fusion
framework so every major sensor contributes ACTUAL signal instead of a neutral
placeholder. Then rebuild the ML corpus + drift engine + warp models + the
candidate verification database (the experience-encoded-as-numbers that the
crash took).

## Acquisition priorities (operator spec, reconciled into code)

- Year tiers (Michigan-Huron low water): 2013,2012,2011,2010,2009,2008,2007...
  2012-13 need Landsat (pre-Sentinel-2). Mandatory Straits downloads: 2012+2013.
- Buckets: historical_low_water (Landsat 2010-13), modern_thermal (2018+, fall),
  fall_zebra_clarity (2018+, Sep-Nov), spring_post_ice (2018+, Apr-Jun N).
- Highest signal-per-TB starter windows: Spring 2012, Fall 2012, Spring 2013,
  Fall 2013, Fall 2019, Fall 2021, Fall 2023.
- Fall (Sep-Nov) is the single highest-priority window: zebra filtration + low
  bio + clarity + thermal gradients + storm plumes overlap.
- Recent sinkings use BEFORE/AFTER bracketing, not low-water buckets.
- Temporal Isolation Gate: keep ~100 DISTINCT environmental states, not 5000
  near-duplicates (saves storage + P100 time).

## Deferred tasks (don't forget)

- **Re-embed nautivecs** (`cesarops-mcp-steered/data/nautivecs_store.json`, last
  built 2026-05-08, 538 records, 768-dim). It does NOT yet include FIELD_NOTES,
  GROUND_TRUTH_LOG, PROJECT_ORIGIN. nautivecs is a *recall index* derived from
  the docs — source of truth is the .md files; the store is regenerable. Needs
  the real 768-dim embedder (was on the killed llama-servers; :5001 currently
  serves Gemma chat, not an embedder; ollama empty). Rebuild when an embedding
  model is back up: `nautivecs-cli index <repo> --endpoint <embedder>/v1`.
  Durability chain: .md docs (must survive) -> nautivecs (regenerable) ->
  agent/LLM/n8n consumer. Open data (Sentinel/Landsat/ICESat/NOAA/NDBC) is the
  same principle for INPUTS: a crash can't take a public archive; re-pullable.

## Data ledger (track acquired + processed — crash-resilient bookkeeping)

- `scripts/data_ledger.py` + `data/ledger/data_ledger.jsonl` (append-only,
  plaintext, regenerable by re-scanning disk). Source of truth for "what do we
  have / what have we processed".
- `scan <roots>` discovers S2 scenes on disk, dedups by scene_id, records
  sensor/year/season/bands/bytes/path/source. `mark --scene --stage --status`
  logs a processing run. `status` = coverage matrix (sensor x year x season).
  `pending --stage poc` = acquired but not yet processed.
- WHY: never re-pull/re-process the same scene; see coverage gaps at a glance;
  feeds the Temporal Isolation Gate (distinct states, not near-dupes). Auth
  (Earthdata for ICESat-2/SAR/SWOT) is NOT the priority — tracking what we have
  IS. SWOT is genuinely sparse (2023+, narrow swath, thin Great Lakes coverage).
- 2026-06-03 baseline: 21 S2 scenes, 19.1 GB. fall 2024=10 (heavy), 2022=7,
  2023=2 + summer 2023=2. Need spread (2019-2021 fall + spring) for 20-distinct floor.

## 2026-06-03  STAC seasonal query silently capped every pull
  Finding: spring (Apr-Jun) pulls returned 0 scenes though STAC had 15+ (May 17
    2023 at 0.0% cloud). Direct STAC query worked; pipeline returned empty.
  Cause:  search_scenes_post queried the WHOLE year with limit=40, STAC returns
    most-recent-first (Oct-Dec), THEN the client-side month_filter [4,5,6]
    dropped all 40. Affected every season filter, not just spring.
  Fix:    tighten the STAC datetime range to the month_filter span before
    querying (+ push eo:cloud_cover into the query body, not just client-side).
  Evidence: spring pull 0 -> 40 scenes (12 perfect-day) after fix.
  Lesson: when a provider returns a paged/limited set, push ALL filters into the
    query; never rely on a post-hoc client filter over a truncated page.

## Data ledger loop is wired end-to-end (2026-06-03)
  run_mission emits ledger_events.jsonl (scene x stage, rc==0 only) into the run
  output dir. `data_ledger.py ingest '<runs>/*/ledger_events.jsonl'` folds them
  (deduped) into the master. Verified: fall detect run -> 30 events (10 scenes x
  poc/bathy/temporal) -> status shows processed tallies, `pending --stage poc`
  shows the 21 unprocessed (spring/summer/older). Coverage now 31 scenes 24.7GB
  across spring/summer/fall 2022-2024 (still 2024-fall heavy; below 20-distinct
  floor for a single season but broadening).

## 2026-06-04  Weather gate wired + storm thresholds were hurricane-class
  Finding: after wiring Open-Meteo weather into scene selection, every scene
    showed days_since_storm=None despite weather_data=true.
  Cause:  WeatherThresholds::default min_storm_wind = 28 m/s (63 mph, hurricane)
    and a 20 m/s (45 mph) secondary — Great Lakes winds run 3-8 m/s, so NO day
    ever classified as a storm. Thresholds were unit/context-wrong.
  Fix:    realistic GL values: calm <=6 m/s, storm >=11 m/s (NWS small-craft)
    OR >=7.7 m/s with >=5mm rain; spring-runoff cutoff 11 m/s. classify_day
    secondary now derives from the threshold, not a hardcoded 20.
  Evidence: days_since_storm populates 10/40; plume score tracks recency
    (dss=3 -> plume 1.00 top priority; dss=10 -> 0.20). Matches spec "delayed
    24-72h post-storm window is best".
  Module: src/weather.rs (Open-Meteo archive client, free/no-auth, 10-day
    lookback) -> full SceneConditions -> scene_score in the download calm-gate.
    Manifest now carries days_since_storm/day_condition/scene_score per scene.

## 2026-06-04  TASK 3: temporal family wired into triple-lock (gate now complete, still can't fire)
  Done: stage_temporal_stack now emits temporal-FAMILY SensorHits (persistence
    z >= triple_lock_temporal_z) that feed fuse_triple_lock as an independent
    family (was: temporal only flowed as a name->z fusion map, never a spatial
    lock candidate). Both local + anchored branches.
  Honest state: triple-lock still emits 0 even at min_locks=2 because:
    (1) only 2 of 4 families AVAILABLE — SAR on raw SLC (won't open), thermal
        absent (no Landsat B10 on disk; concept not in concept.rs);
    (2) the 2 available families DISAGREE spatially — strongest temporal hit
        (z=8.09 @ 45.721,-84.559) is 1638 m from the nearest optical candidate.
  This is correct behavior, not a bug: with 2 families that don't co-locate, no
    lock should form. The gate is structurally ready; it needs real SAR (RTC)
    and/or thermal (Landsat B10) to reach genuine 3-family agreement.
  Next for a real lock: (a) re-pull SAR as RTC GeoTIFF not SLC, OR (b) pull
    Landsat 8/9 B10 thermal + add a thermal concept to the local path. Either
    adds a 3rd independent family.

## 2026-06-04  CRITICAL: data/ is a raid0 symlink — specs/ledger were OUTSIDE git
  Finding: mission specs written to data/missions/ and the ledger in data/ledger/
    were NOT version-controlled — `data/` is a symlink to /mnt/raid0/...-data/data
    (raid0 = NOT redundant). git can't cross the symlink ("beyond a symbolic
    link"). Every spec I wrote for days lived only on raid0.
  Risk: this is the EXACT durability gap the project keeps getting bitten by —
    irreplaceable small files (specs, ledger, labels) on non-redundant storage,
    outside version control. A raid0 failure takes them.
  Fix: git-tracked mission specs live in `missions/` (repo root, REAL dir), NOT
    `data/missions/`. Ledger now lives in-repo at `ledger/data_ledger.jsonl`
    (data_ledger.py LEDGER_DIR default = "ledger", override CESAROPS_LEDGER_DIR).
    Copied all straits_*/detect_* specs + the ledger into the repo.
  RULE: only large re-downloadable DATA (tiles, COGs) goes under data/ (raid0).
    Specs, ledger, labels, notes, code = in the git tree, always.

## 2026-06-04  SENSOR BUILD-OUT: thermal family now real (was absent)
  Added concept_thermal_sink (poc.rs): Landsat TIRS surface-temp z-score, flags
    BOTH cold-sink (deep) and heat-retention (shallow), keeps signed z. Routes
    to triple_lock Thermal family via concept name "thermal_sink".
  Loader: decode_local_band_raw (chip.rs) — no DN->reflectance /10000 rescale
    (that heuristic corrupts thermal DN). concept auto-detects DN vs Kelvin
    (max>1000 => DN, applies K = DN*0.00341802+149). z-score is scale-invariant.
  Fetch: scripts/fetch_landsat_thermal.py via Microsoft Planetary Computer
    (free SAS-sign, no auth). Element84's Landsat lwir11 href is requester-pays
    s3:// (won't download); PC serves signable Azure-blob https. landsatlook=302,
    usgs-landsat https=403 — PC is the working no-auth thermal source.
  Co-location: Landsat overpass dates rarely match S2 dates, so thermal is a
    STANDALONE scan of all *.lwir11.tif in the dir (own date), NOT keyed to S2
    scene IDs. Verified: 25 thermal_sink candidates (z=10) from 1 Landsat tile.
  State: 3 families now flow (optical clarity/glint + thermal + temporal).
    Triple-lock still 0 (need spatial co-location + more thermal scenes), but
    thermal is REAL signal now, not a stub. SAR/SWOT/ECOSTRESS/ATL03 still TODO.

## WHY GDAL-FREE BY DEFAULT (recovered reasoning, 2026-06-04)
The HP host is IVY BRIDGE (AVX, but NO AVX2/FMA3 — those arrived with Haswell).
Two reasons, both load-bearing:

1. HARDWARE / SIGILL. Pre-compiled libgdal (C++) from package managers is often
   cross-compiled with AVX2-optimized codec paths for modern cloud CPUs. On Ivy
   Bridge those paths trigger SIGILL (illegal instruction) crashes. Recompiling
   C++ GDAL from source on an old box to strip the flags is dependency hell.
   Pure-Rust crates (tiff/georaster) defer vectorization to rustc, which
   auto-vectorizes to the EXACT instruction set the chip has — no SIGILL, no
   speed loss. This is why the whole fleet (recycled OptiPlex/HP) runs clean.

2. RAW SENSOR PHYSICS. GDAL is an abstraction engine: it turns raw planetary
   data into a uniform geo grid, and in doing so it:
     - silently strips PRIVATE sub-IFD TIFF tags (proprietary calibration LUTs,
       radar look-up tables, telemetry vectors) that aren't in the GeoTIFF spec;
     - auto-applies orientation/nodata-mask/geometric sensor corrections behind
       your back — destroys raw SLANT-RANGE arrays needed for SAR interferometry
       / custom calibration;
     - can drop precision on complex Real/Imag float bit-depths.
   The pure-Rust `tiff` crate gives byte-level IFD access (iterate every raw tag
   id) and the file exactly as it sits on disk — unrectified pixel stream, true
   sensor values, you control the cast (f32::from_le_bytes). Essential for the
   raw SAR/thermal column-physics this project depends on.

ONE intentional exception: the DRIFT engine uses OpenDrift (Python) for its rich
architecture — not pure Rust. Everything else: pure-Rust, GDAL-free by default.
GDAL remains an OPTIONAL `gdal` feature for convenience on capable hosts only.

## 2026-06-04  RUSTFLAGS target-cpu — the OTHER half of GDAL-free (fleet caveat)
  Old compile note (recovered): RUSTFLAGS="-C target-cpu=native" cargo build --release
  WHY it works: rustc compiles to the BUILD host's full ISA, max auto-vectorization,
    no prebuilt-binary SIGILL. This is what makes pure-Rust as fast as GDAL's C++.
  CRITICAL FLEET CAVEAT: native bakes in the BUILD host's instructions.
    - T440 build host = Xeon Silver 4110 (Skylake-SP, has AVX-512).
    - A native binary built here SIGILLs on the Ivy Bridge HP (AVX only, no AVX2).
  RULE:
    - Build ON the box you run on  -> native is perfect.
    - Build once, distribute to the mixed fleet -> target the OLDEST host:
      -C target-cpu=ivybridge (AVX, no AVX2) is the fleet floor (the HP).
  Helper: scripts/build_satellite.sh {native|ivybridge|haswell|portable};
    default = ivybridge (fleet-safe). GDAL-free is always the default build.

## 2026-06-04  ONE BINARY, EVERY HOST: runtime SIMD dispatch + jemalloc
  Replaces per-host target-cpu builds. Compile with GENERIC flags (NO
  target-cpu=native); at runtime is_x86_feature_detected! routes the hot loops
  through a #[target_feature]-compiled fn for the widest ISA that host has:
  AVX-512 (T440 Xeon 4110) / AVX (HP Ivy Bridge E5 v2) / scalar. The same
  target/release binary copies freely across the mixed fleet — never SIGILLs,
  because the wide instructions only execute on CPUs verified to have them.
  Module: src/simd_dispatch.rs
    - affine_inplace (f32 grid scale+offset, NoData-preserving)
    - complex_scale_inplace + cross_corr_inplace (Array3<Complex32>, Axis0=band,
      Axis1=row, Axis2=col) for raw SAR interferometry; a*conj(b) coherence
      numerator; non-finite => NaN so nodata drops out of the coherence sum.
    - init_thread_pool (rayon = num_cpus::get(), all sockets), active_pipeline().
  Allocator: tikv-jemallocator behind `jemalloc` feature (unix-only), global
    allocator in sat_run — cuts cross-socket allocator contention on dual-Xeon.
  ndarray needs features=["rayon"] for axis_chunks_iter_mut().into_par_iter().
  Verified live on T440: "compute: 32 rayon threads, avx512 vector pipeline,
    allocator=jemalloc"; detection unchanged (Minneapolis 115m, 285 cands).
  Build: `cargo build --release` (generic, fleet-portable) or add
    `--features jemalloc`. Do NOT use target-cpu=native for fleet binaries.
