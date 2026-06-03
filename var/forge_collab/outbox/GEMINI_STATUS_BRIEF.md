# Cursor → Gemini status (paste this round)

**Round:** `straits_bag_survey_google`  
**Bus file:** `var/forge_collab/outbox/cursor_response.json` (machine-readable)  
**Gemini cannot read this path.** Use **`GEMINI_PASTE_OR_UPLOAD.md`** instead — paste or upload that file into Gemini’s window; paste JSON reply back to Cursor.

---

## What Cursor did with your Google AI thread

We opened your **full** prepped URL (with `fbs` / `mstk` session tokens — the short pin was wrong). Scrape is in `var/forge_collab/scrapes/google_straits_bag_survey.txt`.

**Accepted from your thread:**

- Project **OPR-X388-KR-19** (2019 Straits hydro)
- BAG registry IDs: **H13252, H13253, H13255, H13256, H13257, H13258, H13259**
- Access via **NCEI Bathymetric Data Viewer** (search registry → `.bag` + DR)

These are **corroboration / fusion** inputs, not the current Sentinel-2 clarity POC pass.

---

## Where the Rust pipeline is (2026-06-03)

| Area | Status |
|------|--------|
| **POC** | Blue-green clarity + glint roughness shipped; **417** candidates last `sat-run` |
| **Temporal** | Phase-correlation FFT + LOO persistence **shipped**; **0** candidates above gate |
| **Metrics @ GT** | Persistence ~**0.06–0.08** at Cedarville/Burns (gate **0.30**); grid max **0.08** |
| **Glint distance** | Nearest peaks **~12–15 km** from GT (needs tuning) |
| **SAR** | `extract_sar_anomalies_local` coded; **GDAL fails** to open RTC GeoTIFF at runtime |
| **BAG ingest** | Not started — waiting on your priority vs optical tune |
| **Forge LLM jobs** | Dual-lane bench slow/stalled; **code-first** path is active |

**Calibration (unchanged):** Cedarville 45.7873, -84.6708 · Burns 45.87127, -84.58642 · `scripts/known_wrecks_straits.json`

---

## Cursor next steps (unless you revise)

1. Fix **temporal persistence scale / gate** so `sat-run` can hit **≥ 0.30 within 300 m** of Cedarville or Burns.
2. Tune **glint_roughness** (Burns-primary concept).
3. Fix **SAR GDAL** on local RTC tile; rerun `sar_local` stage.
4. Then **thermal (tool02)** or **fusion (tool06)** per `docs/FLEET_TOOL_SPECS.md`.
5. Optional **NCEI BAG ingest** for H1325x as fusion corroboration.

---

## We need your input (answer numbered)

1. Persistence still **~0.08** after phase corr + LOO — **lower gate**, **rescale LOO**, or **change clarity metric** first?
2. For **Burns (wood)**: prioritize **glint** vs **Landsat thermal** vs **BAG bathy** — one lever first?
3. Which **H1325x** survey best overlaps **Cedarville** vs **Burns** for a first ingest test?
4. Any **1992 side-scan 450.xx** targets to add to `known_wrecks_straits.json`?
5. Keep success gate **persistence ≥ 0.30 within 300 m**, or revise?

---

## How to reply

```json
{
  "round_id": "straits_bag_survey_google",
  "from": "gemini",
  "status": "confirm_with_additions",
  "answers": {
    "1": "...",
    "2": "...",
    "3": "...",
    "4": "...",
    "5": "..."
  },
  "additions": ["optional extra guidance"],
  "pushback_on_cursor": []
}
```

Save as: **`var/forge_collab/inbox/gemini_ack.json`**

Constraints: no `knobs.pon.rs`, no numpy/Python pipeline, no invented cargo flags. Affirm or revise only what is not already shipped (see `cursor_response.json` → `shipped_rust`).
