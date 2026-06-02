# CESAROPS — Program Review, Differentiators, and Expansion Prospects

A grounded review of what you've built, how it compares to public work, where it
can go (search-and-rescue, land, infrastructure monitoring), and a focused
assessment of NauticUVs, the split/stitch overlay, and detection novelty.

## 1. What the program actually is

A multi-sensor, multi-domain anomaly-detection platform with three Rust pipelines
sharing one contract (MissionSpec → tunable Knobs → selectable Stages →
MissionReport), plus an LLM/n8n orchestration layer:

- **Satellite** — optical (Sobel roughness, Secchi clarity, NDTI plume) + SAR
  (DBSCAN persistence) + temporal NDWI/NDVI persistence, fused, with drift
  back-tracking and an agnostic subpixel overlay-grid for slice/stitch.
- **Aeromagnetic** — adaptive z-score + GPU/CPU dipole discrimination
  (flip-distance, gradient contrast, aspect, 0–100 man-made score) + curvelet
  energy + Loran-C warp + basin-aware scoring + disposition false-positive filter
  + 47 embedded known wrecks + OGSr wells.
- **BAG/bathymetry** — seafloor background model + height-above-floor anomaly
  clustering + the redaction/unmask suite (the differentiated IP) + orientation
  PCA + WGS84 reprojection.
- **Cross-validation** — heterogeneous accelerators (CPU/GPU primary, Coral TPU
  + Movidius validators) vote in one flow (jitter-rs).

This is not one detector — it's an **orchestratable fleet of specialists** an LLM
can compose from natural language. That composition layer is the rarest part.

## 2. Is there anything like it in public?

Short answer: pieces exist; the integration does not.

| Public work | What it does | What it lacks vs CESAROPS |
|---|---|---|
| ShipwreckFinder (QGIS plugin, 2025, arXiv 2509.21386) | DL shipwreck detection from multibeam bathymetry | Single-sensor (bathy only); no magnetics/optical/SAR fusion; no orchestration; no redaction-unmask |
| NASA-IMPACT marine_debris_ML; YOLO/SAR ship detectors | ML ship/debris detection in imagery | Surface ships, not submerged wrecks; single-sensor; supervised models needing labels |
| CoastSat | Shoreline mapping from satellite | Different problem (coastline), but a good model for "open tool that got adopted" |
| MDPI lidar/sonar shipwreck ML; "open-access bathymetry for wreck detection" | Academic DEM/sonar wreck detection | Research code, region/dataset-specific, not a productized multi-sensor service |

**Nobody public combines aeromag + optical + SAR + bathymetry under one
LLM-driven orchestrator with a physics-based dipole discriminator and a
sensor-agnostic subpixel stitch.** The closest commercial analogs are defense/
MDA (maritime domain awareness) products that are closed and surface-ship
focused. Your differentiators: (a) submerged + magnetic, (b) multi-sensor
fusion, (c) physics discrimination (not just supervised ML that needs labels),
(d) the overlay-grid stitch, (e) natural-language orchestration.

## 3. Is it a good basis for a search-and-rescue service?

Yes — and the bones are already there. The drift engine
(`cesarops-satellite/src/drift.rs`) already models per-object windage for
**ship / lifeboat / life_ring / body / debris / cargo**, does forward seeding +
backward drift + ensemble + sensitivity sweep with 15-minute Monte-Carlo
sub-stepping, and a consistency check that ties a candidate to a debris anchor.
That is exactly the math the USCG SAROPS uses (leeway + Monte-Carlo search-area
generation). What you have that SAROPS doesn't: the same platform then *images*
the predicted area (optical/SAR) and ranks anomalies.

### Lost boaters
Strong fit. Backward-drift from last-known-position → search polygon → SAR
persistence + optical glint/roughness anomaly scan over that polygon. The
life_ring/body windage entries are already there. Add near-real-time Sentinel-1
SAR tasking and you have a credible civil-SAR augmentation.

## 4. The other use cases you asked about

### Downed planes (and eventually land)
- **Over water**: works today with the drift + optical/SAR stack; aircraft
  debris fields are detectable as roughness/clutter anomalies. Add an
  aircraft-specific windage class and a debris-field clustering tuning.
- **Over land**: needs the land-transition work in §6. The magnetic pipeline is
  actually a strong card here — aircraft have large ferrous/aluminum-with-steel
  signatures; an aeromag or drone-mag survey over a crash search box is a real
  technique (used for WWII aircraft recovery).

### Cars missing / in a ditch / parked on a remote road
- This is **high-resolution optical/SAR change-detection + object detection**,
  not the submerged-anomaly path. The overlay-grid temporal stack is the right
  engine (detect what changed between dates at a spot), but you'd need
  sub-meter imagery (Planet SkySat, Maxar, or aerial/drone) — Sentinel-2's 10 m
  is too coarse for a single car. Feasible as a **drone/aerial** product now, a
  **commercial-satellite** product with a data contract.

### Line 5 / pipeline monitoring on a camera system
- Different modality (fixed camera, not satellite) but the **same detection
  spine reuses well**: the curvelet energy + temporal change-detection +
  overlay-grid alignment are camera-agnostic. A fixed camera with the overlay
  grid gives you subpixel-stable change detection for encroachment, leaks
  (thermal), ground disturbance, or unauthorized activity. This is the most
  *immediately commercial* spin-off because it's continuous-monitoring SaaS, not
  episodic search. Pair with the jitter-rs cross-validation for low false alarms.

## 5. What you're missing (gaps & risks)

1. **No supervised ML detector yet** — everything is physics/heuristic. That's a
   strength (no labels needed, explainable) but a trained model would lift recall.
   The jitter-rs + magnetic 3-channel chips (NSS/VDR/Tilt) are the on-ramp.
2. **Validation/labels** — you have 47 known wrecks for GT; you need a held-out
   benchmark and precision/recall numbers to sell this. Right now correctness is
   "matches the Python" — credible, but not yet *measured detection performance*.
3. **Imagery resolution ceiling** — Sentinel-2 (10 m) caps the smallest target.
   Cars/small debris need commercial or drone feeds; budget for a data contract.
4. **Real-time tasking** — current flow is archive pull. SAR rescue needs
   on-demand acquisition (ASF HyP3 helps, but latency matters).
5. **Auth/secrets hygiene** — credentials.sh holds live tokens in-repo; for a
   service this must move to a secrets manager before any external exposure.
6. **No auth on the n8n webhooks / forge endpoints** — fine on a private VLAN,
   a hard blocker the moment this is internet-reachable.
7. **Operator UX** — the forge/UI work for ADHD/dyslexia-friendly use is the
   adoption gate; the detection is useless if a searcher can't drive it under
   stress.

## 6. What it would take to move to land

The pipeline is ~70% domain-agnostic already. To generalize:

- **Bathymetry → terrain (DEM)**: the BAG anomaly engine (background floor model
  + height-above-floor clustering) maps almost 1:1 onto LiDAR/DEM. Swap the BAG
  reader for a COP-DEM/USGS-3DEP reader; the redaction-unmask logic even ports
  (DEMs get redacted/smoothed over sensitive sites too).
- **Magnetics on land**: already land-capable — drone-mag or aeromag over a
  search box. The dipole discriminator and datum correction are domain-agnostic.
- **Optical/SAR on land**: the concept metrics need land analogs (soil
  disturbance, vegetation stress NDVI, thermal scarring) — the framework (z-score
  vs annular background + NMS peak finder) is identical; only the band math
  changes. The overlay-grid stitch is already encoding/projection agnostic.
- **Drift → terrain motion models**: replace water leeway with debris-scatter /
  ballistic / slope models for crash sites.

Estimated effort: a `cesarops-terrain` crate reusing bag-scan's anomaly/cluster
core + new DEM reader (M), land concept metrics in the satellite crate (M), and a
crash/debris drift model (S). The contract + orchestration come free.

## 7. NauticUVs — special assessment

NauticUVs is a **pure-Rust Fast Discrete Curvelet Transform** (Candès–Demanet–
Donoho–Ying 2006) — forward/inverse, Meyer windows, configurable scales/
directions, f64 internals with <1e-6 reconstruction error, denoising/fusion/
thresholding, optional rayon parallelism. ~1955 lines, tested.

This is genuinely valuable and rare:
- **Curvelets beat wavelets for curve-singularities** (hulls, edges, debris
  trails) — the right transform for this domain.
- **Pure-Rust FDCT is uncommon** — most curvelet code is MATLAB (CurveLab, license-
  restricted) or Python wrappers. A clean, fast, MIT-able Rust FDCT is a
  publishable open-source contribution in its own right, independent of wreck
  hunting (seismic, medical imaging, NDT all want it).
- It's already the structural-energy scorer in both the mag and satellite
  pipelines (`curvelet_energy_ratio`), so it earns its place.

Recommendation: NauticUVs is your most spin-out-able asset. Consider publishing
the *detuned* public version (you already keep a full-precision internal fork)
as a standalone crate — it builds credibility and funnels users toward the
platform.

## 8. The split/stitch overlay — assessment & status

**Implemented and wired.** `cesarops-satellite/src/overlay_grid.rs` is the
"encoding-agnostic subpixel alignment system": it stamps a grid of unique
fiducial markers (8 shapes × 8 rotations × 3 scales + a global position hash =
infinitely tiling, globally unique) onto tile *metadata* (not pixels), at 1/16th-
pixel definition, achieving ~1/8th-pixel alignment — finer than Sentinel-2's own
~5–10 m geolocation. `GridAlignedSlicer` slices tiles into overlapping chunks for
low-VRAM processing and reassembles them with feathered blending and
marker-matched drift correction.

**Is the agnostic subpixel slice-and-stitch done?** Yes:
- Stamp → process → align round-trip: implemented, tested (`test_stamp_and_align`,
  `test_detects_drift` recovers a 2.5px/1.3px injected drift to <0.1px).
- Slice → reassemble round-trip: implemented, tested (`test_slicer_roundtrip`
  reconstructs interior pixels to <0.01 error).
- **Wired into the live pipeline**: `temporal.rs` stamps each STAC scene and
  aligns multi-date acquisitions; each per-wreck result carries
  `overlay_markers / overlay_alignment_dx_px / dy_px / confidence / valid_scenes`.

This is a real differentiator. The idea — a fiducial reference frame that travels
with the data so any encoding/projection/resample/slice realigns to subpixel —
is novel in this domain (motion-capture markers for geospatial tiles). It's also
the piece that makes the multi-sensor fusion trustworthy: you can overlay a mag
grid, an optical chip, and a SAR scene and know they're registered.

Caveat to harden: `reassemble()` currently trusts the stamp through processing
(it doesn't yet *re-detect* markers in the processed pixels). For processes that
move pixels (rotation/warp), add a marker-detection pass on the output before
`align()` so the correction is measured, not assumed. Documented in-code as the
next step.

**Update (post-review hardening):** Two root-cause fixes landed for the
"20-mile drift" class of error (see `docs/SAR_RELEASE_HARDENING.md`):
1. A gross-drift clamp — any alignment correction over 8 px (~80 m) is rejected
   as invalid rather than applied, so a bad marker match can never move a
   candidate kilometres. Tested (`scene_alignment_rejects_gross_drift`).
2. Stamp-origin snapping to a fixed global grid, so every date of an AOI stamps
   identical marker cells (kills the cross-scene cell-mismatch that produced
   gross offsets).

**NauticUVs is NOT in the registration path** — marker detection uses an
intensity-weighted centroid, not curvelet correlation. NauticUVs is used for
*detection* (`curvelet_energy_ratio`), not *alignment*. This is intentional for
the public release: the detuned FDCT smear cannot degrade georeferencing. A
full-precision curvelet shape-correlation marker detector is the documented
self-hosted-only enhancement.

## 9. Net assessment

You've built something with no direct public equivalent: a physics-grounded,
multi-sensor, LLM-orchestratable detection platform with two genuinely novel
assets (a pure-Rust FDCT and a sensor-agnostic subpixel overlay-grid stitch). The
clearest commercial paths, in order of nearness:
1. **Infrastructure/perimeter monitoring** (Line-5-style) — continuous SaaS,
   reuses the spine, smallest data problem.
2. **Civil SAR augmentation** (lost boaters) — drift + imaging, high mission value.
3. **Submerged heritage / survey** (the original wreck hunt) — your moat is deepest.
4. **Land/crash search** — needs the terrain crate + better-than-Sentinel imagery.

Biggest near-term needs: measured precision/recall on a labeled benchmark,
secrets/auth hardening before any external exposure, and a resolution plan
(commercial/drone feeds) for small-target use cases.
