# WreckHunter / Straits — Cursor status for Gemini (paste or upload this whole file)

**Round ID:** straits_bag_survey_google  
**From:** Cursor (repo: wreckhunter2000-1)  
**Date:** 2026-06-03  

**Instructions for you (Gemini):** Read this entire document. Reply in the chat with the JSON block in Section 5 filled in (user will paste it back into Cursor). Do not invent file paths, `knobs.pon.rs`, numpy, or fake `cargo` subcommands.

---

## 1. Context we accepted from the Google AI thread

- **Project:** OPR-X388-KR-19 (2019 Straits of Mackinac hydrographic survey)
- **BAG registry IDs:** H13252, H13253, H13255, H13256, H13257, H13258, H13259
- **Access:** NOAA NCEI Bathymetric Data Viewer — search by registry (e.g. H13252) → download `.bag` grids + Descriptive Report (DR)
- **Role in pipeline:** Bathymetry is **fusion corroboration**, not the primary Sentinel-2 optical clarity pass running today on local NFS tiles.

**Ground-truth calibration wrecks:**

| Name | Lat | Lon | Notes |
|------|-----|-----|-------|
| Cedarville | 45.7873 | -84.6708 | Steel, deep — thermal/SAR strong |
| Burns | 45.87127 | -84.58642 | Wood, ~34 m — glint/roughness primary |

Six wrecks in `known_wrecks_straits.json` (dive-verified).

---

## 2. What Cursor already shipped (Rust — do not re-spec as Python)

- **poc.rs:** `BLUE_GREEN_Z_CAP = 4.0`, edge margin 3 px, `concept_blue_green_clarity`
- **poc.rs:** `concept_glint_roughness` (Sobel on local B02/B03 variance) — FLEET tool 5
- **temporal.rs:** LOO persistence map per scene date
- **phase_corr.rs + temporal.rs:** FFT phase correlation before persistence (`rustfft`, ε=1e-12, parabolic sub-pixel)
- **sar.rs:** `extract_sar_anomalies_local` + mission stage `sar_local` (code complete)
- **Mission:** `straits_local_run.json` on NFS optical dirs (clear + 2022 + 2023)

**Real knobs (if you suggest tuning):** `cesarops-satellite/src/types.rs` — `poc_zscore_threshold`, `poc_max_candidates`, `poc_min_separation_px`, `poc_downsample_max_dim`. Temporal: `PERSISTENCE_MIN = 0.3`, `ANOMALY_Z = -1.5` in `temporal.rs`.

**Verify command:**  
`/data/cargo-target/release/sat-run --spec data/missions/straits_local_run.json --root /data/cesarops/satellite_data`

---

## 3. Latest `sat-run` results (2026-06-03)

| Metric | Value |
|--------|-------|
| POC candidates | 417 (includes glint) |
| Temporal candidates above gate | **0** |
| Persistence at Cedarville | ~0.059 |
| Persistence at Burns | ~0.063 |
| Grid max persistence | ~0.081 |
| Gate | 0.30 |
| Nearest glint peak to Cedarville | ~15 km |
| Nearest glint peak to Burns | ~12 km |
| Nearest clarity peak to Cedarville | ~3.2 km |
| Nearest clarity peak to Burns | ~3.9 km |
| SAR local stage | GDAL failed to open RTC GeoTIFF (`GDALOpenEx` NULL) |

Phase correlation runs (example): most scenes dx/dy ≈ 0; one scene had large shift (dx≈-608, dy≈485) — may need outlier rejection.

---

## 4. Cursor planned next steps (revise if you disagree)

1. Fix temporal persistence **scale or gate** so we can hit **≥ 0.30 within 300 m** of Cedarville or Burns.
2. Tune **glint_roughness** for Burns (wood).
3. Fix **SAR GDAL** on local RTC tile; validate cluster within 500 m of Cedarville.
4. **Landsat thermal (B10)** or **fusion.rs** multi-sensor weights after optical/temporal improves.
5. Optional: ingest NCEI BAG for H1325x surveys as fusion corroboration.

Dual-lane Forge (Mixtral/Gemma/Qwen) is **secondary** — implementor mode is **code first**, LLM advisory only.

---

## 5. Questions — please answer numbered in your reply JSON

1. Persistence still **~0.08** after phase corr + LOO: should Cursor **lower PERSISTENCE_MIN (0.3)**, **rescale LOO output**, or **change the clarity metric (B02/B03)** first?

2. For **Burns (wood, 34 m)**: prioritize **glint roughness** vs **Landsat thermal** vs **BAG bathy** — which **one** lever first?

3. Which **H1325x** survey ID best overlaps **Cedarville** vs **Burns** for a first BAG ingest test (one ID each)?

4. Any **1992 side-scan `450.xx`** targets we should add to ground-truth beyond the current six wrecks?

5. Keep success criterion **persistence ≥ 0.30 within 300 m** of GT, or propose a revised gate?

---

## 6. Reply template — copy this JSON into your chat response (filled in)

```json
{
  "round_id": "straits_bag_survey_google",
  "from": "gemini",
  "status": "confirm_with_additions",
  "answers": {
    "1": "YOUR ANSWER",
    "2": "YOUR ANSWER",
    "3": "YOUR ANSWER",
    "4": "YOUR ANSWER",
    "5": "YOUR ANSWER"
  },
  "additions": [
    "Any extra physics or metric guidance for Cursor"
  ],
  "pushback_on_cursor": []
}
```

Use `status`: `confirm` | `revise` | `confirm_with_additions`.

**User:** paste Gemini’s JSON reply into Cursor chat (or save as `gemini_ack.json` and say “process collab ack”).

---

## 7. Constraints (push back if Cursor violates)

- No `knobs.pon.rs`, no `cesarops-inference`, no numpy/`np.clip` in satellite crate
- No invented `cargo run -- clear-water` or similar
- Do not re-implement FFT phase correlation in Python — already in `phase_corr.rs`
- BAG/H1325x is corroboration until optical/temporal metrics improve (unless you explicitly reprioritize)
