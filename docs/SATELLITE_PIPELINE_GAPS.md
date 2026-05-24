# Great Lakes satellite pipeline — gap map (Lake MI + Straits)

Last audit: 2026-05-24. Goal: end-to-end run of all forge satellite tools.

## Tool inventory (forge `/tool/*`)

| Tool | Role | Status |
|------|------|--------|
| `weather_window` | Pre-mission wx check | Wired in orchestrator; needs live bbox |
| `sat_mission` | JSON stages (download→target→stack→validate→report) | Works dry-run; live needs Earthdata |
| `sat_read_mission_report` | Read mission/validation JSON | OK |
| `download_satellite_window` | Standalone 5-source downloader | OK; not in sat_mission chain (duplicate of stage download) |
| `detection_health` | Triple-lock workers | OK (c2 :5580) |
| `detection_scan` | Submit tiles to triple-lock | Wired; tiles lack `image_b64` from CSV |
| `detection_poll` | Poll scan job | Wired after scan |
| `scan_region` | Legacy scan | Separate from sat pipeline |
| `magnetic_dipole_detect` | Aeromagnetic | Not in satellite spec missions |

## Mission specs

| Mission | JSON | Status |
|---------|------|--------|
| Straits GT validation | `straits_known_wreck_validation.json` | Exists; dry-run OK |
| Lake Michigan north | `lake_michigan_north_wreck_validation.json` | **Missing** — add |
| Lake Michigan south | — | Not defined (use preset bbox from universal_downloader) |

## Stage gaps (`sat_mission_orchestrator.py`)

| # | Gap | Severity | Owner task |
|---|-----|----------|------------|
| G1 | `gt_wreck_names` in spec ignored; bbox returns wrong 4 wrecks | High | Filter GT by spec list |
| G2 | `dry_run` skips `target_known` → no CSV → `validate_gt` all `no_data` | High | Emit fixture CSV on dry-run |
| G3 | `detection_scan` tiles from CSV have empty `image_b64` | High | Chip fetcher or placeholder PNG per lat/lon |
| G4 | Live `download` needs NASA Earthdata / `.env` credentials | High | Document + verify `credentials.sh` |
| G5 | No combined runner script (LM + Straits + orchestrator) | Med | `run_great_lakes_satellite.sh` |
| G6 | n8n JSON uses old `spec_path` under repo not `/codebase/projects` | Med | Fix paths |
| G7 | Vision workers on CPU sim only until `VISION_MODE=gpu` on c2 | Med | Ops, not code |
| G8 | `temporal_stack` skipped in dry-run; untested live | Med | Live run with chips |
| G9 | Orchestrator `weather_window` optional; sat spec has own download | Low | Document |
| G10 | `forge_tools.json` / n8n only Straits; no LM webhook | Med | Add LM mission to n8n |

## End-to-end flow (target)

```
weather_window → sat_mission (download, target_known, temporal_stack, validate_gt, report)
  → sat_read_mission_report
  → detection_health → detection_scan (tiles w/ image_b64) → detection_poll
  → MTP review (report.review)
```

## Fleet dispatch order

1. **Coder (Gemma :5001)** — G1, G2, G3, G5 code fixes
2. **Reviewer (MTP :5571)** — review diff + test plan
3. **Coder** — revisions from review
4. **Reviewer** — polish + sign-off
