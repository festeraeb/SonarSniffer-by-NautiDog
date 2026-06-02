# Mission request for Forge — dynamic GPU slot watchdog

**Submitted via:** thinker dispatch test (routing preset + manual spec)  
**Replaces:** `scripts/mission_service_watchdog.sh` fixed triple-stack behavior

## Request

Implement **`scripts/mission_gpu_slot_watchdog.sh`** (or Forge-managed equivalent) that:

1. Reads **`/data/cesarops/logs/gpu-slot-heartbeat.json`** (or `GET /cluster/gpus` snapshot) with per-slot:
   - `gpu_uuid`, `port`, `model_path`, `host`, `timestamp`
2. On each tick, for each slot where `endpoint_online` is false but a **last-known** `model_path` exists:
   - `pkill` / `fuser` that **port only**
   - wait for VRAM drop
   - relaunch **same** `model_path` on **same** `port` + `cuda` device (from snapshot)
3. **Never** call `cesarops2_triple_gpu_llama.sh start` as a blind preset.
4. Write heartbeat snapshot when slots are healthy (merge Forge `/cluster/gpus` + `cmdline_model`).

## Acceptance

- After kill -9 llama, watchdog restores prior model on correct port within 2 ticks.
- `cluster_config.toml` `[[gpu]]` rows match `gpu_uuid` from snapshot.
- Forge routing unchanged unless operator applies a preset.

## Not in scope

- Polisher lane (`:5010` CPU Coder-Next) — separate.
