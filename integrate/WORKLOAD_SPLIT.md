# Laptop-dump integrate workload (3-way split)

## P100#0 `:5001` — Qwen3.6-35B-A3B MoE (coder)
- **Role:** Bulk `PORT_TO_PIPELINES` Python → Rust modules
- **Output:** `integrate_out/split_rust/p1000/rust__*.md`
- **Run:** `nohup python3 scripts/integrate/run_split_integrate_rust.py >> /tmp/split_integrate_rust.log 2>&1 &`
- **Resume:** skips existing `.md` files with valid `## Verdict` (>400 bytes)

## P100#1 `:5002` — Qwen3.5-9B MTP (reviewer)
- **Role:** Smaller scripts + MERGE-style ports
- **Output:** `integrate_out/split_rust/p1001/rust__*.md`

## Agent (hardest → `cesarops-inference/src/integrate/`)
| Python source | Rust module | Status |
|---------------|-------------|--------|
| `analyze_fuel_leaks.py` | `fuel_leak.rs` | done |
| `cesarops_engine.py` | `tile_zscore.rs` | done |
| `prioritized_pull_v2.py` | `great_lakes.rs` | done |
| `multi_sensor_scan.py` / `lake_michigan_full_scan.py` | `sensor_scan.rs` | done |
| `cuda_direct.py` | `cuda_stats.rs` | done |
| `integrated_forensic_scan.py` / `full_basin_scan.py` | `forensic_scan.rs`, `full_basin.rs` | done |
| `triple_lock_fusion.py` | `triple_lock.rs` | done |
| `daily_satellite_pull.py` | `daily_pull.rs` | done |
| `cesarops_cli.py` | `cli_orchestrator.rs` | done |
| `cesarops_agent_gui.py` | `agent_presets.rs` | done |

## Queue merge
```bash
python3 scripts/integrate/merge_dispatch_plans.py
```

ARCHIVE_STUB items are not ported unless promoted.
