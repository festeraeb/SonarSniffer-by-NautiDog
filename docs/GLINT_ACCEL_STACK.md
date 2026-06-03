# Glint detection — three tiers (Rust + TPU + Movidius)

Full multi-sensor stack (weather, aeromag lane, drift, SDB bathymetry): `docs/TOOL_STACK_GEMINI_BRIEF.md`.

Burns-type wood wrecks need **surface glint / current-roughness**, not deep-column NIR. The fleet uses three complementary layers:

## Tier 1 — Rust (`cesarops-satellite`) — shipped

| Piece | Module | Role |
|-------|--------|------|
| Per-scene POC | `poc.rs` `concept_glint_roughness` | Sobel(local B02/B03 variance) |
| Multi-date LOO | `temporal.rs` `glint_persistence_map.json` | Phase-corr + baseline residual + persistence |
| GT anchor | `temporal.rs` | Validation emit near Cedarville/Burns |

Run: `sat-run` with `temporal_stack` stage on NFS `straits_optical_*`.

## Tier 2 — Coral Edge TPU (cesarops2 / ML350e)

| Endpoint | Default | Role |
|----------|---------|------|
| `POST /infer` | `http://10.0.0.201:8092` | Fast scout: glint/hydrocarbon/thermal confidence on chip PNG |
| Validator (optional) | `http://10.0.0.201:8190` | Remote vote into jitter-rs |

Spec: `docs/full_sensor_scan_spec.md`, `sovereign-cloud/src/pipeline.rs` (scout pass forwards to `TPU_SERVER_URL` when no local TPU).

## Tier 3 — Movidius NCS2 + jitter-rs (T440)

| Endpoint | Default | Role |
|----------|---------|------|
| `POST /jitter` | `http://10.0.0.61:8180` | Lock-3 primary; Movidius `myriad_fp16` + tract/heuristic |
| Nomad job | `infra/nomad/jobs/t440-movidius-jitter.nomad.hcl` | Production bring-up |

Jitter consumes **thermal_timeseries** band labels (scene folder band names) + lat/lon/depth → signature vote (`device: movidius_ncs2` in artifacts).

See `docs/ACCELERATORS_FLEET.md`, `config/fleet_manifest.json` → `jitter_rs`, `coral_jitter_validator`.

## Straits batch runner

```bash
# NFS Sentinel-2 blue/green tiles — one accel scan per scene (green chip → TPU)
bash scripts/role_bench/run_straits_glint_accel.sh

# Or manual (limit 3 scenes for smoke test):
python3 scripts/role_bench/run_accel_scan_files.py \
  --out var/role_bench/straits_glint_accel \
  --geotiff-mode scene --limit-scenes 3 \
  --prefer-band green \
  --tpu http://10.0.0.201:8092 --jitter http://10.0.0.61:8180 \
  /mnt/raid0/wreckhunter2000-1-data/data/straits_optical_clear/sentinel2_aws
```

Outputs: `accel_scan_packet.json` + per-scene `*_jitter.json` (Movidius vote metadata).

## Ground truth (tool proving)

**Primary:** dive-verified wrecks from `scripts/known_wrecks_straits.json` (Michigan Preserves / Wikipedia). Mission `straits_local_run.json` sets `gt_min_confidence: 1.0` so only `dive_verified` entries in the AOI bbox are used for `validate_gt`.

```bash
bash scripts/role_bench/prove_tools_preserve_gt.sh
```

Use Cedarville (steel/SAR), Eber Ward, M. Stalker, Minneapolis, etc. to score POC / temporal / SAR / glint+accel — not BAG downloads.

## BAG (independent verification tool)

`cesarops-bag-scan` and `run_straits_bag_scan.sh` are **optional** bathymetry corroboration. They are **not** on the `sat-run` critical path.

## Fusion order (wood / glint targets)

1. Rust glint LOO persistence (primary metric gate)
2. TPU scout confidence on B03 chip (trigger / corroboration)
3. Movidius jitter-rs vote (hardware validator; cross-check false glint)
4. Optional BAG scan if a `.bag` is already on disk

Do **not** route glint-only work to P100 LLM ports — use `:8180` / `:8092` accelerators per `config/fleet_manifest.json`.
