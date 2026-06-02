# Orchestrator action plan (n8n + Rust pipeline)

Working list derived from satellite/detection analysis. Execute in order; archive superseded files instead of deleting.

## Phase A — Rust tool surface

| # | Task | Status |
|---|------|--------|
| A1 | `sat_mission` tool → `sat_mission_orchestrator.py` | done |
| A2 | `sat_read_mission_report` tool → read `mission_report.json` / `validation_report.json` | done |
| A3 | Repo path resolver (`/codebase`, `/mnt/data-external`, NFS) | done |
| A4 | Register tools in `execute()` + unknown-tool message | done |

## Phase B — Sequential Rust pipeline

| # | Task | Status |
|---|------|--------|
| B1 | `OperatorScenario`: `spec_path`, `knobs`, `stages`, `dry_run`, `pipeline_mode` | done |
| B2 | `build_satellite_spec_plan()` when `spec_path` set | done |
| B3 | `dispatch_modules_sequential()` with `PipelineContext` chaining | done |
| B4 | Weather uses `days_back`; sat mission → read report → detection_health/poll | done |
| B5 | Sequential default when `spec_path` set | done |

## Phase C — n8n + webhook

| # | Task | Status |
|---|------|--------|
| C1 | `POST /webhook/satellite` dedicated intake | done |
| C2 | Extend `POST /webhook/mission` with spec/knobs/days_back | done |
| C3 | n8n workflow: HTTP → `/orchestrator/execute` (archive old executeCommand JSON) | done |

## Phase D — GPU / worker policy

| # | Task | Status |
|---|------|--------|
| D1 | `scripts/p100_cycle.sh` — stop Kobold on :5001/:5002 for tests, restore after | done |
| D2 | `scripts/test_orchestrator_pipeline.sh` — plan + execute dry-run mission | done |
| D3 | cesarops2: use real GPUs if idle; else CPU sim workers (`cesarops2_research_lab.sh`) | done |
| D4 | Detection :5580 — CPU sim on 5570/5572/8080 when vision GPUs busy | done (T440) |

## Phase E — Build & verify

| # | Task | Status |
|---|------|--------|
| E1 | `cargo build --release -p cesarops-forge-v2` | done |
| E2 | Deploy via `scripts/deploy_forge.sh` (`$CARGO_TARGET_DIR`) | done |
| E3 | E2E: `POST /orchestrator/execute` straits spec `dry_run: true` | done |
| E4 | n8n: `missions/n8n_forge_satellite_pipeline.json` | done |

## Phase F — Research backlog (after green)

- Wire weather-filtered dates into `run_download`
- Targeting CSV → `detection_scan` tiles
- `buoy_analog` seiche tags → STAC date picker
- Temporal stack v2 per-chip bands on dual P100
- Forge `MeanOfMeans` or call `temporal_stack_engine`
