# Dynamic GPU slot watchdog

Replaces blind `cesarops2_triple_gpu_llama.sh` / `p100_gemma_r1_dual.sh` recovery inside `mission_service_watchdog.sh`.

## Behavior

1. **Record** — For each watched port (Forge `routing_state.json` + local `llama-server` listeners), when `/v1/models` is healthy, snapshot:
   - `gpu_uuid`, `port`, `model_path`, full `launch_argv` from `/proc/pid/cmdline`
2. **Restore** — When a watched port is down, `pkill`/`fuser` that port only, then relaunch the **last** `launch_argv` or `model_path` from the heartbeat file.
3. **Never** forces a fixed triple-stack preset when `GPU_SLOT_DYNAMIC=1` (default).

## Files

| Path | Role |
|------|------|
| `scripts/gpu_slot_heartbeat.py` | Core tick: snapshot + restore |
| `scripts/gpu_slot_watchdog.sh` | Standalone tick with flock + busy policy |
| `/data/cesarops/logs/gpu-slot-heartbeat.json` | Canonical heartbeat store |
| `var/gpu-slot-heartbeats/latest.json` | Repo mirror (NFS) |

## n8n / mission watchdog

`mission_service_watchdog.sh` still runs `n8n-watchdog.sh` first, then calls `gpu_slot_heartbeat.py tick` instead of triple-stack start.

Set `GPU_SLOT_DYNAMIC=0` to revert to legacy preset scripts.

## Keep Qwen2.5-Coder-14B on `:5002` (not Qwen3.6 MoE)

Watchdog **dynamic restore** replays the last heartbeat. If `:5002` ever ran Qwen3.6, the next recover brings MoE back.

```bash
# Stop all LLM watchdog reloads (recommended while coding on P100s)
bash scripts/forge_llm_watchdog_off.sh

bash scripts/start_qwen14_coder_p100.sh
bash scripts/pin_qwen14_heartbeat_5002.sh

# When you want auto-recover again (after pinning heartbeat)
bash scripts/forge_llm_watchdog_on.sh
export GPU_SLOT_PINNED_RESTORE_5002=/path/to/repo/scripts/start_qwen14_coder_p100.sh
```

MoE think/polish stays on **CPU `:5010`** only (`start_qwen36_moe_cpu_laneb.sh`).

## Operator

```bash
# One-shot tick (record + recover if policy allows)
bash scripts/gpu_slot_watchdog.sh tick

# Inspect last snapshots
bash scripts/gpu_slot_watchdog.sh show

# Force recover one port (after a healthy run populated heartbeat)
python3 scripts/gpu_slot_heartbeat.py recover-port --port 5200

# Allow recover while Forge is busy
touch /tmp/c2-rebind.ack
```

## First-time bootstrap

Heartbeats are empty until slots have been healthy once. Start your fleet layout manually once:

```bash
bash scripts/cesarops2_fleet_roles.sh start   # c2
bash scripts/p100_gemma_r1_dual.sh start      # t440
```

The next watchdog tick records snapshots; later failures restore those models on the same ports.
