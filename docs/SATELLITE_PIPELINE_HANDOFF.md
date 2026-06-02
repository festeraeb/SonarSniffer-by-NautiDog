# Satellite Pipeline — Completion Handoff Spec
Date: 2026-06-02. Author: T440 agent (out of credits). For: next agent.

## MISSION
Get `cesarops-satellite` (`sat-run`) running on the ALREADY-DOWNLOADED Straits
Sentinel-2 tiles (offline, no STAC/network), parallelized with rayon, then on
the freed P100 GPUs. Goal: see if the fully remote-sensed optical pipeline
INDEPENDENTLY flags the masked wreck the BAG pipeline found — a true cross-sensor
confirmation on an unknown wreck.

## THE TARGET (what success looks like)
- BAG pipeline found an intact, upright wreck in a NOAA-masked zone:
  **45.87127°N, -84.58642°W**, peak 113ft / floor 145ft, **32ft relief**,
  measured ~110-130ft × 30-40ft beam.
- Primary hypothesis: **Robert Burns** (126ft wood barquentine, "E of Bois Blanc").
- If the optical pipeline produces a candidate within ~300m of that point from
  Sentinel-2 alone, that's confirmation. (300m because optical centroid at 10m
  resolution + plume advection is coarse.)

## CALIBRATION TARGET (prove the pipeline first)
**Cedarville** (45.7873°N, -84.6708°W) — 588ft steel freighter, shallow, large.
User mapped it successfully with the ORIGINAL python tool ("big and easy").
**The pipeline MUST light up Cedarville before we trust any Burns result.**
Run Cedarville first as the known-good. If it doesn't flag Cedarville, the
detector is broken — fix that before interpreting anything else.
Other shallow control wrecks in `scripts/known_wrecks_straits.json`:
Eber Ward, William Young, M. Stalker, Elva.

## DETECTION PHYSICS (critical — informs concept weighting)
1. NOT direct hull imaging. Detection = water-column disturbance from the wreck
   rising toward the thermocline, read optically in BLUE-GREEN at ~180ft
   apparent depth. Multi-year/multi-date stack captures the plume advected by
   the Straits current (wreck "appears" to move a few hundred ft between dates).
2. **Material matters:** thermal cold/heat-sink is STRONG on STEEL (Cedarville),
   WEAK on WOOD (Burns). Wood wrecks signal via PHYSICAL OBSTRUCTION:
   zebra-clarity, current-driven surface glint modulation, shadow/texture.
   → For the Burns (wood): down-weight thermal, up-weight zebra_clarity + glint.
   → For Cedarville (steel): thermal works, use all concepts.
3. **Depth = 34m is beyond SDB bottom-detection** (~2-3× Secchi ≈ 20-30m in
   Straits). Do NOT expect bottom reflectance off the hull. The signal is the
   COLUMN clarity/plume, not the bottom. (See punchlist B1.1.)
4. **Bands:** use B02 (blue) + B03 (green) — water-penetrating. B04 (red) and
   B08 (NIR) barely penetrate; only useful for surface (plume/glint). Per user,
   red+NIR can be dropped for this deep non-fuel target.
5. **Glint/currents are SIGNAL not noise** — Straits currents over a 32ft hull
   modulate surface roughness/glint, visible blue-green. (Punchlist B1.2: build
   a blue-green glint/current-roughness concept; not done yet.)

## DATA ON DISK (all downloaded, ready)
- Sentinel-2 (tile 16TFR = Straits), per-band GeoTIFFs:
  - `data/straits_optical_clear/sentinel2_aws/` — 9 scenes Sept 2024
  - `data/straits_optical_2022/sentinel2_aws/` — 4 scenes
  - `data/straits_optical_2023/sentinel2_aws/` — 4 scenes
  - Band file naming: `S2{A,B}_16TFR_<date>_0_L2A.<band>.tif`
    where band ∈ {blue,green,red,nir,nir08,swir16,swir22,scl}
    **Map: B02=blue, B03=green, B04=red, B08=nir**
- Landsat: `data/straits_multisensor/usgs/LC0{8,9}_*.tar.gz` (6 bundles, need untar)
- SWOT: `data/straits_multisensor/podaac/swot/*.nc` (120)
- ICESat-2: `data/straits_multisensor/podaac/icesat2/ATL13_*.h5.h5` (120)
- SAR (Sentinel-1 RTC): `data/straits_sar/sar/rtc_S1A_*.tif` (1, 455MB, Aug 2024)
NOTE: `data/` is a symlink to /mnt/raid0 (raid0 = NOT redundant). Real path
resolves fine for reading.

## WORK ALREADY DONE (don't redo)
1. `sat-run` binary builds: `cargo build --release -p cesarops-satellite`
   (binary at /data/cargo-target/release/sat-run). Default build does NOT enable
   gdal feature — see STEP 1.
2. Mission spec written: `data/missions/straits_local_run.json` (stages
   target_known, poc_aoi, temporal_stack, report; paths point at local tiles).
3. **Local band loader ADDED but UNTESTED**: `chip.rs::decode_local_band(path,
   bbox, target_px)` + `bbox_to_pixel_window()` — reads a local Sentinel-2
   GeoTIFF via GDAL, reprojects WGS84 bbox→tile UTM, windows, resamples,
   DN→reflectance scales. **Requires the `gdal` feature to compile.**
4. P100s are FREE (llama-servers killed; watchdogs off so they won't reload).
   2× Tesla P100-16GB, 32GB total. CUDA 12 runtime + libcuda present, cudarc
   0.17.8 cached, but NO nvcc (can't compile .cu; use cudarc PTX/cuBLAS).

## STEP-BY-STEP TO FINISH

### STEP 1: Build with gdal feature, fix decode_local_band compile
```
cargo build --release -p cesarops-satellite --features gdal 2>&1 | tail
```
The new `decode_local_band`/`bbox_to_pixel_window` in chip.rs may have GDAL 0.17
API mismatches (check `read_as` signature: `(window_origin:(isize,isize),
window_size:(usize,usize), out_size:(usize,usize), resample)` returns Buffer;
`.data()` → &[f32]). `spatial_ref()`/`CoordTransform`/`AxisMappingStrategy`
usage mirrors `geo.rs` in cesarops-bag-scan (working reference). Fix until clean.

### STEP 2: Wire decode_local_band into a new offline POC path
In `poc.rs`, add `run_poc_aoi_local(bbox, scene_dir, knobs, known_wrecks)` that:
- Globs `scene_dir/*.blue.tif` to enumerate scene IDs.
- For each scene, loads B02(blue)+B03(green) via `decode_local_band` (skip
  red/nir for the deep target; load them only if a concept needs them).
- **rayon: `scenes.par_iter().map(|s| load+score).collect()`** — scene-level
  parallelism (each scene independent). rayon is already a dep, currently
  UNUSED anywhere (0 par_iter calls in the crate — that's the slowness).
- Runs `concept_zebra_clarity(b02, b04=green-as-proxy or load red, ...)` — NOTE
  zebra_clarity currently wants b02+b04(red); for a deep target consider b02+b03.
  Read the concept; adapt bands or add a b02/b03 clarity variant.
- Cross-references against known_wrecks (cross_reference() already exists).
In `mission.rs::stage_poc_aoi`, branch on knob `use_local_scenes`: if set, call
`run_poc_aoi_local(scene_dir = paths.download_dir)` instead of the STAC
`run_poc_aoi`. The spec already sets `use_local_scenes:true` and download_dir.

### STEP 3: Run Cedarville FIRST (calibration)
Make a spec variant with bbox tight around Cedarville (45.787,-84.671) and
gt_wreck_names=["Cedarville"]. Run:
```
/data/cargo-target/release/sat-run --spec data/missions/<cedarville>.json \
  --root /data/cesarops/satellite_data
```
EXPECT: a candidate within ~300m of 45.7873,-84.6708. If yes → pipeline works,
proceed. If no → debug the concept/band loading before trusting anything.

### STEP 4: Run the full Straits AOI, check the Burns location
Run `data/missions/straits_local_run.json`. In the report candidates[], look for
any within ~300m of **45.87127, -84.58642**. Multi-date stack (temporal_stack
stage) is where the plume-advection signal lives — make sure ≥3 dates load.

### STEP 5 (after it works): GPU acceleration
The hot path is per-pixel band math + multi-date z-score on ~10980² f32 arrays.
rayon (step 2) gets 16-32× on the 32 Xeon cores — likely ENOUGH. Only go GPU if
still too slow. GPU plan: cudarc, hold the multi-date stack in P100 VRAM (f16,
~4GB for 9 scenes×2 bands×120Mpx), z-score across date axis in one kernel.
No nvcc → use cudarc with prebuilt PTX or cuBLAS. This is optional polish.

## ACCEPTANCE
- Cedarville lights up (calibration passes).
- Report generated with candidates ranked by composite_score.
- Check whether any candidate falls near the Burns target → the headline result.
- Hand back the candidate list + distances to known wrecks.

## FILES
- `cesarops-satellite/src/chip.rs` (decode_local_band — added, untested)
- `cesarops-satellite/src/poc.rs` (add run_poc_aoi_local + rayon)
- `cesarops-satellite/src/mission.rs` (branch stage_poc_aoi on use_local_scenes)
- `data/missions/straits_local_run.json` (spec, exists)
- `scripts/known_wrecks_straits.json` (ground truth incl. masked_target + Burns)
- `docs/SATELLITE_AEROMAG_PUNCHLIST.md` (B1, B1.1, B1.2 — SDB + glint concept)

## DO NOT
- Don't push to git (local WIP commits only).
- Don't reduce nauticuvs to 32-bit (native f64 build is intentional).
- Don't work military/defense framing — humanitarian SAR + survey only.
- Don't trust a Burns hit unless Cedarville calibration passed first.


---

## ADDENDUM: c2 (ML350e) disk + NFS issues (2026-06-02 late)

### c2 /data is 100% FULL (456/481GB, 0 avail)
Root cause: **354GB model weights** in `/data/cesarops/models` + 66GB in `/data/models`.
These are LOCAL COPIES of LLM weights that are ALREADY accessible from T440 via
NFS (`/mnt/t440/models`, mounted read-only). They're redundant.

**To clean:** remove the redundant local model copies that are accessible via NFS:
```bash
# On c2 — CONFIRM the NFS mount resolves first:
ls /mnt/t440/models/  # should show the same models
# Then remove the local redundant copies:
rm -rf /data/cesarops/models  # 354GB
rm -rf /data/models           # 66GB
# Result: /data goes from 100% → ~8% used (33GB bathymetry left)
```
CAUTION: only do this if the T440 NFS export stays up. If T440 goes down, c2
loses model access. For production, keep at least ONE critical model local.

### Sentinel-2 tiles NOT on c2 (they're on T440 raid0)
The tiles live at T440: `/mnt/raid0/wreckhunter2000-1-data/data/straits_optical_*`.
The repo symlink `data/ → /mnt/raid0/wreckhunter2000-1-data/data` only resolves
on T440 (or any machine that mounts T440's raid0 at that path).

**Option A (recommended): Add NFS mount on c2 for raid0:**
```bash
# On c2:
sudo mkdir -p /mnt/raid0
sudo mount -t nfs 10.0.0.61:/mnt/raid0 /mnt/raid0
# Add to /etc/fstab for persistence:
# 10.0.0.61:/mnt/raid0 /mnt/raid0 nfs defaults,_netdev 0 0
```
Then the repo `data/` symlink resolves naturally on c2.

**Option B: Use the existing NFS mount paths directly:**
c2 already mounts T440's `/data` at `/mnt/t440/data`, but the satellite tiles
are on raid0 not /data. The tiles ARE also reachable via:
```
/mnt/t440/repo/data/straits_optical_clear/sentinel2_aws/
```
(because /mnt/t440/repo is NFS of the whole repo, and NFS follows the symlink
on the server side). **Test this path first before adding a new mount.**

### The "first set in the same spot" (duplicate data on c2)
`/data/cesarops/bathymetry` (33GB) on c2 — this is likely the early unfiltered
NCEI BAG download that ran on c2 before we killed it and cleaned up. It's
already synced to T440's /data drive. Can be removed from c2 if confirmed to be
a subset of what T440 has at `/data/cesarops/bathymetry/nos_surveys/` (325GB).

### Mixtral :5211
Not running / not responding on LAN. Was likely on the llama-servers we killed
on T440. If needed for fleet grading, restart on T440 after the satellite
detection run completes (P100s are currently free for compute; models can reload
later since watchdogs are off).
