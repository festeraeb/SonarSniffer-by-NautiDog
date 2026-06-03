# Pipeline Defaults Inventory — sensors, scans, scores, logic

Source-of-truth snapshot of every sensor, detector/scan, and its DEFAULT scoring
constants + logic, for external spec/tuning. Generated from
`cesarops-satellite` source (commit `42c35044`).

All values are the wired DEFAULTS; nearly all are overridable via mission-spec
`knobs` (serde key in parens). Z-scores are signed unless noted; "fitness"/
"score" terms are 0–1 unless noted (concept composite is 0–10).

---

## 1. SENSORS / DATA SOURCES

| Sensor | Role | Bands used | Archive start | Pass | Notes |
|--------|------|-----------|---------------|------|-------|
| Sentinel-2 (L2A) | optical relief/clarity/glint | B02 blue, B03 green, B04 red, B08 NIR, B11 SWIR16 | 2017 | day only (sun-sync ~10:30) | primary; 10 m |
| Landsat 5/7/8/9 | thermal (TIRS) + optical, reaches record lows | B10 thermal, +optical | 1984 | day + night thermal | only sensor for 2012–13 lows |
| ICESat-2 (ATL13) | laser altimetry, limited deep penetration | — | 2018 | — | depth assist beyond SDB |
| SWOT | surface water elevation | — | 2023 | — | seiche/current context |
| Sentinel-1 (SAR/RTC) | steel-mass backscatter persistence | C-band σ0 | 2014 | day+night (active) | needs RTC GeoTIFF, NOT raw SLC |
| NDBC / GLOS buoys | wave/wind calm-gate + turbidity | WVHT, WSPD | per-station | — | 20 GL stations, 5 lakes |
| Aeromag | magnetic anomaly correlation | — | survey-dependent | weather-independent | ferrous mass |
| BAG (NOAA bathymetry) | sounding/redaction unmask | depth grid + uncertainty | survey-dependent | — | mask = its own candidate |

---

## 2. OPTICAL CONCEPTS (detectors) — default scoring

Each concept scores a candidate 0–10. Anchored mode (`target_known`) uses an
annular signal-vs-background z-score; AOI sweep mode (`poc_aoi`) uses peak
clustering on a z-map.

### Shared annular geometry (concept.rs)
| Param | Default | knob |
|-------|---------|------|
| signal inner radius | 150 m | `chip_signal_m` |
| background annulus inner | 350 m | `chip_bg_inner_m` |
| background annulus outer | 1200 m | `chip_bg_outer_m` |
| STAC fetch window radius | 1500 m | `chip_scene_radius_m` |
| hit z-score min (|z| counts as a hit) | 1.5 | `hit_zscore_min` |

### Anchored composite score (per concept)
```
hr_score   = hit_rate * 10           # hit_rate = n_hits / n_scenes
z_score10  = clamp(best_zscore / 4 * 10, 0, 10)
score      = 0.5 * hr_score + 0.5 * z_score10   # + optional curvelet bonus (≤1.5)
```

### Concept families, signal direction, season, family bucket
| Concept | Bands | Signal dir | Default season months | Triple-lock family |
|---------|-------|-----------|------------------------|--------------------|
| shadow_roughness | B08 (NIR) Sobel | dark | 3,4 | optical |
| zebra_clarity | B02/B04 Secchi proxy | bright | 7,8,9,10 | optical |
| sediment_plume | B04/B03 NDTI | turbid | 4–10 | optical |
| blue_green_clarity | B02/B03 (deep target) | dark (low ratio = anomaly) | local-path | optical |
| glint_roughness | B02+B03 Sobel(local var) | high gradient | local-path | optical |
| thermal cold/heat-sink | B10 (Landsat) | cold (deep) / cycling (shallow) | depth-driven | thermal |
| sar_temporal_persistence | C-band σ0 | bright (steel) | any | sar |
| temporal_persistence | multi-date z | persistent | any | temporal |

### POC AOI sweep (poc.rs)
| Param | Default | knob |
|-------|---------|------|
| z-score peak threshold | 2.5 | `poc_zscore_threshold` |
| blue-green z cap | 4.0 | (const) |
| NMS min separation | per-concept (15/10/10 px) | `poc_min_separation_px` |
| max candidates / concept | 25 | `poc_max_candidates` |
| downsample max dim | 2000 px | `poc_downsample_max_dim` |
| cross-ref nearby radius | 2000 m | `xref_nearby_radius_m` |

---

## 3. THERMAL REGIME LOGIC (env_conditions.rs)

Set by DEPTH vs SUNLIGHT, not day/night of the same wreck.
- photic/lit-layer depth by month: spring(3-5)=130 ft, summer deeper (~220 ft).
- `thermal_regime(depth_ft, month)`:
  - depth ≥ photic_depth → **AlwaysCold** (persistent cold sink; any pass).
  - depth < photic_depth → **SunCycling** (image afternoon peak / pre-dawn).
- Material weighting: thermal STRONG on steel, WEAK on wood (wood → down-weight
  thermal, up-weight clarity + glint).

---

## 4. SAR (sar.rs)

| Param | Default | knob |
|-------|---------|------|
| backscatter anomaly sigma | 3.0 (`DEFAULT_SAR_SIGMA`) | — |
| DBSCAN eps | 0.5 | `dbscan_eps` |
| DBSCAN min samples | 5 | `dbscan_min_samples` |
| SAR cluster composite | persistence*10 (+ NASA fusion avg if present) | — |

NOTE: current Straits SAR input is raw SLC; needs RTC GeoTIFF to open. SAR
family currently produces 0 hits → triple-lock can't reach 3 families yet.

---

## 5. CANDIDATE FUSION (fusion.rs)

Composite (per spatial candidate):
```
composite = W_CONCEPT*best_concept(0-10)
          + W_TEMPORAL*temporal_contrib(0-10)
          + W_DRIFT*drift_bonus(0-10)
          + known_proximity * W_KNOWN
```
| Weight | Default |
|--------|---------|
| W_CONCEPT | 0.6 |
| W_TEMPORAL | 0.3 |
| W_DRIFT | 0.1 |
| W_KNOWN (known-wreck corroboration bonus, points) | 4.0 |
| KNOWN_PROXIMITY_RADIUS_M | 300 m |
| MIN_EMIT_SCORE | 3.0 |
| min_score to emit candidate | 0.2–4.0 | `min_score` |

temporal_contrib = clamp(|z|/4, 0,1) * 10.

---

## 6. TRIPLE-LOCK GATE (triple_lock.rs) — multi-sensor agreement

A candidate is trustworthy only when ≥ min_locks INDEPENDENT sensor families
co-locate. Families: thermal / sar / optical / temporal. Per-family z threshold:

| Param | Default | knob |
|-------|---------|------|
| thermal lock |z| | 2.5 | `triple_lock_thermal_z` |
| sar lock z | 2.5 | `triple_lock_sar_z` |
| optical lock z | 2.5 | `triple_lock_optical_z` |
| temporal lock z | 2.0 | `triple_lock_temporal_z` |
| spatial fuse tolerance | 300 m | `triple_lock_tolerance_m` |
| min distinct families | 3 | `triple_lock_min_locks` |

(Defaults are the operator's hand-tuned `triple_lock_fusion.py` values, now
overridable. lock confidence = avg|z| * lock_level.)

---

## 7. SCENE SELECTION — calm gate, conditions, composite priority

### Buoy calm gate (buoy.rs)
| Param | Default |
|-------|---------|
| calm wave height | ≤ 0.30 m |
| calm wind speed | ≤ 5.0 m/s (~10 kt) |
| data fallback | historical stdmet → realtime2 (~45 d) |
| missing data | "unknown" (neutral, NOT rough) |

### intent_fitness(intent, conditions) → 0–1 (env_conditions.rs)
Per-intent weighted sum of: calm, clear_water(turbidity), cloud_ok,
after_storm_window. Examples (weights):
- Bathymetry: 0.30 calm + 0.25 clear + 0.20 cloud + 0.25 post-storm(3-7 d)
- ZebraClarity: 0.30 calm + 0.30 clear + 0.20 cloud + 0.20 post-storm(3-7)
- SedimentPlume: 0.55 storm-recency(0-2 d) + 0.45 cloud
- ThermalFront: 0.35 cloud + 0.40 post-storm(1-3) + 0.25 calm
- DeepWreck: 0.35 calm + 0.25 clear + 0.25 cloud + 0.15 post-storm(2-7)

### Composite download_priority (operator chat-paste formula)
```
priority = (thermal + clarity + stratification + calm_water + post_storm + seasonal) / 6
```
- seasonal_score: fall(9,10)=1.0, Nov=0.8, spring(4,5)=0.9, Jun=0.7, summer(7,8)=0.6, else 0.2
- post_storm: 24–72 h (d 1-3)=1.0 best, 0–24 h=0.7, ≤7 d fades, long-calm=0.2
- stratification: summer(7,8)=1.0 peak; turnover months penalised; recent wind mixing degrades

### Temporal Isolation Gate
- state_vector = [wind, wave, cloud, turbidity, season-phase] each ~0–1.
- keep a scene only if Euclidean state-distance ≥ min_distance (~0.15–0.25)
  from every already-kept scene → ~100 DISTINCT states, not 5000 near-dupes.

### Scan counts (scan_plan.rs)
| Param | Default |
|-------|---------|
| MIN_SCENES (begin scans) | 20 |
| TARGET_SCENES | 100 |
| BATCH_SIZE | 30 |
| stack_window_days | 20 | `stack_window_days` |

---

## 8. YEAR / SEASON SELECTION (lake_levels.rs)

### Michigan-Huron low-water ranking (lowest-first)
`2013, 2012, 2011, 2010, 2009, 2008, 2007, 2006, 2005, 2003`
(2012-13 need Landsat; S2 archive clamps to 2017+.)

### Acquisition buckets (acquisition_buckets, overridable with pinned years)
| Bucket | Tier | Sensor | Years (auto) | Season |
|--------|------|--------|--------------|--------|
| historical_low_water | 1 | landsat | 2010–2013 | spring_fall |
| modern_thermal | 1 | landsat,sentinel2 | 2018–now | fall |
| fall_zebra_clarity | 2 | sentinel2 | 2018–now | fall |
| spring_post_ice | 2 | sentinel2 | 2018–now | post_ice_out |

### Season month windows (basin-aware; northern = Straits/Superior/Mich-Huron)
| Profile | Northern | Erie/Ontario |
|---------|----------|--------------|
| post_ice_out (default) | 4,5,6 | 3,4,5 |
| fall | 9,10,11 | 9,10,11 |
| spring_fall | 4,5,6,9,10,11 | 3,4,5,9,10,11 |
| summer | 7,8,9 | 7,8,9 |
| open_water | 5–11 | 4–11 |

### Scan modes (scan_plan.rs)
- BeforeAfter (recent sinking): brackets sink date → 2 yr PRE + sink→now POST.
  `recent_sinking_years(sensor, sink_year, pre_n)` returns (pre, post) subsets.
- Historical: best available years per sensor by intent.

### Scan intents (year ranking)
low_water_wreck | recent_sinking | zebra_clarity | event_response | generic

---

## 9. KNOB DEFAULTS (types.rs Knobs)
| knob | default |
|------|---------|
| sensors | "sentinel2" |
| concepts | "all" |
| max_cloud | 20.0 |
| min_score | 4.0 |
| scan_intent | "low_water_wreck" |
| season_window | "post_ice_out" |
| pass_time | "both" |
| auto_low_water_years | 4 |
| stack_window_days | 20 |
| temporal_persistence_z | 2.0 |
| triple_lock_min_locks | 3 |

---

## 10. KNOWN GAPS (honest status)
- SAR family: input is raw SLC, won't open → 0 SAR hits → triple-lock can't
  reach 3 families on optical alone.
- Thermal: not wired into the LOCAL offline path yet (only STAC anchored path).
- Local POC currently runs only blue_green_clarity + glint_roughness; plume +
  thermal not yet in the local loop.
- ML: LightGBM wreck_classifier.pkl (50 feats, AUC 0.868) + 426-sample dataset
  recoverable; geology hard-negative (E-of-Elva) staged but not yet folded in.
