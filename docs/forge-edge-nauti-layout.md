# Forge edge + NautiInferer on cesarops2

Two layouts, one switch away from the other.

## Roles

| Layout | Forge | NautiInferer | Use when |
|--------|-------|--------------|----------|
| **Edge (default on c2)** | `127.0.0.1:9100` | `127.0.0.1:8099` | Daily inference, integrate, Cake worker on c2 |
| **T440 backup** | `10.0.0.61:9100` | optional remote | NFS/T440 conductor, P100 presets, “way back” |

P100 GPUs are **not** in the edge base routing (`avoid_p100` in API). Load `p100-integrate` or `cake-fleet` only via preset when Zaya testing is done.

## Quick start (cesarops2)

```bash
# 1) LLMs (Qwen/Gemma dual script)
bash scripts/cesarops2_qwen36_gemma4_dual.sh start

# 2) Forge edge + edge routing
bash scripts/cesarops2-forge-edge.sh start

# 3) NautiInferer coordinator
bash scripts/nauti-inferer-c2.sh start
```

## Routing switch

```bash
# Home layout (2060/1070 local)
bash scripts/forge-routing-switch.sh edge

# Snapshot “way back” to T440-style endpoints
bash scripts/forge-routing-switch.sh backup

# Named preset from cluster_config.toml (not applied at boot)
bash scripts/forge-routing-switch.sh preset cake-fleet
bash scripts/forge-routing-switch.sh preset c2-edge-default

# Save current active routing as backup file
bash scripts/forge-routing-switch.sh save-backup
```

Files under `cesarops-forge-v2/routing/`:

- `routing_state.edge.json` — active after `edge`
- `routing_state.backup.t440.json` — active after `backup`
- `mode_state.edge.json` — UI metadata for edge

## Environment

| Variable | Edge default |
|----------|----------------|
| `FORGE_V2_DIR` | repo `cesarops-forge-v2` on NFS |
| `FORGE_URL` | `http://127.0.0.1:9100` |
| `FORGE_ROUTING_STATE` | optional override path |
| `NAUTI_LISTEN_PORT` | `8099` |

## Failover

If c2 Forge is down, point NautiInferer at T440:

```bash
export FORGE_URL=http://10.0.0.61:9100
bash scripts/nauti-inferer-c2.sh stop && bash scripts/nauti-inferer-c2.sh start
```

NautiInferer still uses static fallback nodes (2060/1070) when Forge `/cluster/gpus` fails.

## Cake: GPU + system RAM (cesarops2)

```bash
# MoE test (35B, expert-offload + RAM master) — can run beside llama if VRAM tight use FREE_LLAMA=1
bash scripts/cake/start-cake-c2-hybrid.sh start

# 72B — needs ~50GB+ free RAM + GPU headroom; frees llama on 2060/1070
FREE_LLAMA=1 USE_70B=1 WAIT_WORKERS=180 bash scripts/cake/start-cake-c2-hybrid.sh start

bash scripts/cake/start-cake-c2-hybrid.sh status
curl -s http://127.0.0.1:8081/v1/models
```

## Rebuild Forge after routing.rs changes

On a machine with Rust toolchain:

```bash
cd /mnt/t440/codebase/repos/wreckhunter2000-1/cesarops-forge-v2
cargo build --release
```
