# CESAROPS — full tool stack review (paste into Google AI)

We are proving detectors against **Michigan Preserves dive-verified wrecks** (not BAG downloads). Please read each tool’s math and reply with rankings, gaps, and tuning order.

Full reference: repo `docs/TOOL_STACK_GEMINI_BRIEF.md` (sections 0–11).

---

## Three pipelines (do not merge blindly)

1. **Satellite (`sat-run`)** — optical, temporal, SAR, satellite SDB bathymetry, weather-gated scene pick, preserve `validate_gt`.
2. **Aeromagnetic (separate)** — `cesarops-aeromagnetic-worker` + `pipelines/mag/erie_central_aeromag_orchestrator.py`. **Only when aeromag grids exist.** Best for **steel** (dipole, adaptive z, curvelet). Not for wood hulls. Not blocking satellite bench.
3. **Verification** — optional NOAA BAG scan; Coral TPU + Movidius jitter for glint pattern-lock.

**Drift model** (`drift.rs`) is **not** a detector — it back-projects surface plumes/glint along wind+current when multi-date stacks span different weather. Used with the **20-day low-cloud stack** and phase-corr alignment.

---

## Weather end — picking satellite days

Before any pixel math:

- **Open-Meteo** daily wind/precip/cloud → `Calm | Storm | PostStorm(1-3) | SpringRunoff | Transitional`
- **ScanIntent** (`lake_levels.rs`): low-water years vs recent-sinking vs zebra-clarity vs event — *not* always lowest water (S2 only back to 2017; 2012 lows need Landsat)
- **Thermal regime** from wreck **depth vs seasonal photic depth** (~90–220 ft): deep = always cold sink; shallow = sun cycling → pick afternoon or pre-dawn thermal passes
- **Spring runoff** = clean sediment plume + higher current (good for plume intent; bad for clarity)
- **Post-storm** = first calm days after storm (suspended sediment over wreck)
- Scene rank ≈ **low cloud × condition_suitability(intent)**

Question: For **steel at ~100 ft** (Cedarville) vs **wood at ~100 ft** (Burns proxy), which `ScanIntent` + day conditions should we weight highest?

---

## Satellite bathymetry (recovered BathymetryMapper → `bathymetry_map.rs`)

**Multi-pass SDB:** each clear Sentinel-2 pass contributes depth proxy; **turbidity (NDTI) down-weights** muddy passes; fuse weighted mean depth + relief gradient.

- Stumpf: `Z = m0 − m1·ln(B02)` capped at **2.5× Secchi** (~20–30 m in Straits)
- At **~100 ft (34 m)** hull: expect **shoal/flank mapping & column turbidity**, not hull reflectance
- Lyzenga log-ratio B02/B03 as fallback

Question: How should we calibrate m0/m1 using shallow preserve wrecks? What turbidity cutoff for dropping a pass?

---

## Other satellite tools (math one-liners)

| Tool | Math | Steel ~100 ft | Wood ~100 ft |
|------|------|---------------|--------------|
| Blue-green clarity | ln(B02)/ln(B03) z-score | Low | **High** |
| Glint roughness | Sobel(var B02,B03) | Medium | **High** |
| Temporal LOO | baseline residual + phase-corr + persistence | Medium | **High** |
| SAR RTC | local ±3σ clusters + DBSCAN persist | **High** | Low |
| Thermal B10 | Landsat K, cold/heat by regime | **High** | Low |
| TPU/Movidius | infer + jitter vote | Corroborate glint | Corroborate glint |

---

## Aeromagnetic (separate pipeline)

Adaptive background + dipole discrimination + curvelet (flight-line geometry). **Use when mag data available — steel wrecks only.**

Question: Should we require aeromag hit within X m of satellite candidate before promoting steel targets?

---

## Drift (separate physics)

`u = current + 0.03×wind`; storm-phase forward/backward; analog storm match. Explains **offset** between surface detection and seabed GT — not a score.

Question: Max offset (m) we should allow when validating temporal candidates against preserve coords?

---

## Your task

1. Rank tools for **steel ~100 ft** vs **wood ~100 ft** in the Straits.
2. Flag **missing math** or wrong physics in any row above.
3. Propose **tuning order** (top 5 knobs/thresholds).
4. Any **pushback** on our SDB depth limit or weather rules?

Reply with JSON only:

```json
{
  "round_id": "straits_tool_stack_review",
  "from": "gemini",
  "tool_rankings": { "steel_100ft": [], "wood_100ft": [] },
  "weather_picks": { "steel": "", "wood": "" },
  "missing_math": [],
  "tuning_priority": [],
  "pushback_on_cursor": []
}
```
