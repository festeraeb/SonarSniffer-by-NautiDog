# Research pipeline backlog (post–Rust orchestrator)

Verified on T440: sequential `POST /orchestrator/execute` with `spec_path` runs
`wx-window → sat-mission → sat-reports → det-health → det-wreck` (dry-run OK).

## Next experiments (pipeline should surface these)

1. **Weather → download dates** — pass `recommended_dates_*` from `weather_window` into `sat_mission` knobs / `universal_downloader`.
2. **Targeting CSV → detection tiles** — export STAC chips as `image_b64` for `:5580/scan`.
3. **Erie seiche timing** — wire `buoy_analog.analyze_seiche()` into date picker for `sediment_plume` concept.
4. **Temporal stack v2** — per-chip B03/B08 on dual P100 (`tiles_per_gpu_window` knobs).
5. **Pass 6–7 promotion** — call `lake_erie_scan` mussel clearspot + calm/post_storm displacement from Rust optional module.
6. **GT wreck name alignment** — `straits_known_wreck_validation.json` lists Cedarville/Eber Ward; dry-run loaded Gilcher/Parnell/Bradley/Meteor from bbox filter — reconcile `gt_wreck_names`.

## Deploy note

Build output: `$CARGO_TARGET_DIR/release/cesarops-forge-v2` (default `/data/cargo-target`).
Use `bash scripts/deploy_forge.sh` before restarting forge on :9100.
