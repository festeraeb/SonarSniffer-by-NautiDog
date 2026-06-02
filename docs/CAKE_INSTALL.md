# Cake fleet install (upstream `cake-cli`)

Upstream [evilsocket/cake](https://github.com/evilsocket/cake) is the **idle audit** inference stack (`cake_fleet` mode). It is **not** `cake_kv` in `cesarops-inference` — see [CAKE_VS_CAKE_KV.md](CAKE_VS_CAKE_KV.md).

## Install (crates.io preferred)

On **T440** (master) and **cesarops2** (augment worker):

```bash
cd /codebase/repos/wreckhunter2000-1
bash scripts/install_cake_fleet.sh
```

Or from T440 via n8n (no SSH): see [N8N_FLEET_DISPATCH.md](N8N_FLEET_DISPATCH.md)

```bash
bash scripts/fleet-n8n-dispatch.sh cesarops2 install_cake
```

This runs:

1. `rustup` if needed  
2. `cargo install cake-cli --features cuda` (crates.io), or `--git https://github.com/evilsocket/cake.git` on failure  
3. Symlink `/opt/cesarops/cake/bin/cake`  
4. Create `/etc/cesarops/cake-cluster.key` (shared secret for LAN workers)

Requirements: NVIDIA driver + CUDA toolkit **≥ 12.2** for CUDA builds (cesarops2: **cuda-toolkit-12-6** at `/usr/local/cuda-12.6`).

**RTX 2060 (Turing, sm_75):** `cargo install --features cuda` needs `CUDA_COMPUTE_CAP=86` at build time (bf16 WMMA MoE kernels do not compile for sm_75). `install_cake_fleet.sh` sets this automatically.

## CLI reality (no `cluster start`)

Cake **does not** implement `cake cluster start|stop`. Fleet scripts use:

| Action | Command |
|--------|---------|
| Start fleet (default) | `scripts/cake/start-fleet-cluster.sh` — `cake worker` + `cake master` with `--cluster-key` |
| Stop fleet | `scripts/cake/stop-fleet-cluster.sh` — kill PID files + `pkill` |
| One-shot audit | `cake worker` / `cake master` with `--cluster-key` (see `cesarops-cake-audit-loop.sh`) |
| Manual layers | `cake worker` / `cake master … --topology scripts/cake/topology_fleet_idle_35b.yml` |

Set `CAKE_FLEET_MODE=topology` only after workers are listening on the ports in the topology file.

## Enable after pipeline

```bash
sudo touch /etc/cesarops/fleet-mode.enabled
sudo cp systemd/cesarops-activity-watch.* /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now cesarops-activity-watch.timer
```

**Do not** enable while Forge reports a `running` mission or while satellite pipeline jobs need `:5001/:5002/:5200/:5571`.

## Models

| Phase | Model | Notes |
|-------|--------|--------|
| 2 | `Qwen/Qwen3.6-35B-A3B-Instruct` | MoE idle audit |
| 6 | `Qwen/Qwen2.5-72B-Instruct` | **Full dense 72B** (not distill) — `USE_70B=1` |
| alt | `Qwen/Qwen3-32B` | **Full dense Qwen3** — `USE_QWEN3_FULL=1` |

**Not on this fleet:** `deepseek-ai/DeepSeek-V3` / `DeepSeek-R1` (~685B MoE total). Distill checkpoints (`DeepSeek-R1-Distill-*`) are smaller but not “full” parent models.

| Phase | Topology |
|-------|----------|
| 35B | `topology_fleet_idle_35b.yml` |
| 70B hetero | `topology_fleet_hetero_70b.yml` + `start-fleet-hetero-70b.sh` (P100#1 + c2 P106/2060/1070 + T440 RAM; **no laptop**) |
| 70B cluster-key | `start-fleet-70b-no-p100.sh` (one worker per host only) |

**Pascal / Turing (P100, GTX 1070, RTX 2060):** do **not** use the default `sm_86` build at runtime (PTX JIT fails). Build **native** caps:

```bash
bash scripts/cake/install_cake_pascal_fleet.sh   # sm60 T440, sm75 + sm61 on cesarops2
# or per host: bash scripts/cake/install_cake_for_cap.sh 60
```

**Dual P100 72B** (P106 out, llama freed from `:5001`):

```bash
export SKIP_P106=1 CAKE_DUAL_P100=1
bash scripts/cake/start-fleet-hetero-70b.sh
bash scripts/cake/start-master-when-c2-ready.sh
```

Topology: `topology_fleet_hetero_70b_dual_p100_no_p106.yml` (ports `10133`/`10131`/`10132` + c2 `10129`/`10130`).

**ThinkPad M2200:** excluded from Cake 72B fleet (WiFi/Tailscale latency). Kobold/validator on `:5571` only. See `topology_fleet_hetero_70b_with_laptop.yml.example` if wired LAN later.

```bash
/opt/cesarops/cake/bin/cake pull Qwen/Qwen3.6-35B-A3B-Instruct
```

Pull 35B + two 70B-class models (fleet script):

```bash
bash scripts/cake_pull_fleet_models.sh
# 70B only: SKIP_35B=1 bash scripts/cake_pull_fleet_models.sh
# via n8n: bash scripts/fleet-n8n-dispatch.sh t440 cake_pull_models
```

Default 70B pulls: `Qwen/Qwen2.5-72B-Instruct`, `Qwen/Qwen2.5-72B`.

**When 72B is online** — audit `cesarops-inference` (staged, never auto-applied):

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/cake/run-qwen72b-inference-fix.sh
# or background: nohup bash .../run-qwen72b-inference-fix.sh >> ~/.cache/cesarops/cake_fleet.log 2>&1 &
```

Prompt brief: `cesarops-inference/fleet_prompts/CAKE_QWEN72B_INFERENCE_ENGINE_FIX.md`  
Output: `research_log/staged_fixes/inference_fix_<timestamp>.md`

## Excluded GPUs

Never shard **intake GTX 1070 :5599** or **laptop M2200** (WiFi) into Cake. **70B hetero** uses c2 **P106 + 2060 + 1070** on `:10128–10130`. **35B idle** topology still uses 2060 only.
