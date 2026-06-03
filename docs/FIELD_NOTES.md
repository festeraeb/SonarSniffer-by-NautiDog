# Field Notes — wreck-logic / why the code does what it does

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
