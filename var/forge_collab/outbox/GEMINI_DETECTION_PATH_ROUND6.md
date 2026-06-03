# CESAROPS — detection path completion (Cursor + you)

We are **finishing the detection path without Forge on the hot path**. Twin **P100s** will later run satellite math (FFT, z-stacks, scene-parallel POC); Xeons orchestrate `sat-run`. Forge LLM lanes wind down.

**Reference:** `docs/TOOL_STACK_GEMINI_BRIEF.md`, `docs/DETECTION_PATH_ROADMAP.md`, `docs/SATELLITE_P100_OFFLOAD.md`

**Cursor will run:** `bash scripts/role_bench/run_detection_path.sh` → `detection_path_report.json` + preserve `validate_gt`.

---

## Current shipped stack

- **GT:** dive_verified Michigan Preserves (`gt_min_confidence: 1.0`)
- **Optical:** blue-green clarity, glint roughness, temporal LOO + phase-corr, dynamic gate near GT
- **SDB:** `bathymetry_map.rs` multi-pass Stumpf/Lyzenga, turbidity-weighted
- **SAR:** local RTC extract (GDAL open still failing on our tif)
- **Accel:** TPU + Movidius; `fuse_glint_accel.py`
- **Separate:** aeromag (steel), drift (offset physics), BAG (optional verify)

---

## Questions for you

1. **Detection path priority:** Order these tunings for max preserve-GT hits: temporal gate, glint NMS, SAR fix, fusion material weights, bathy calibration?
2. **Steel vs wood @ ~100 ft:** Confirm tool ranking (we have round 5 ask — align with detection_path_report when Cursor posts numbers).
3. **P100 offload:** Which modules first — `phase_corr` FFT, per-scene POC, temporal LOO stacks, or bathy fuse? Any risk on 16 GB Pascal for our chip sizes (~2048²)?
4. **Forge sunset:** Agree LLM should not sit in the compile-test loop — only spec/review?
5. **Missing math** in any tool in the brief?

Reply JSON:

```json
{
  "round_id": "straits_detection_path",
  "round": 6,
  "from": "gemini",
  "detection_priority": [],
  "p100_offload_order": [],
  "tool_rankings": { "steel_100ft": [], "wood_100ft": [] },
  "missing_math": [],
  "forge_sunset": "agree|revise",
  "pushback_on_cursor": []
}
```
