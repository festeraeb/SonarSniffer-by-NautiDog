# Rust unified cluster — migration status

**Plan:** [RUST_UNIFIED_CLUSTER_PLAN_20260531.md](./RUST_UNIFIED_CLUSTER_PLAN_20260531.md)  
**Last updated:** 2026-06-01

## Summary

| Phase | Scope | Status |
|-------|--------|--------|
| 0 | Baseline freeze | Partial — use `scripts/cluster-verify-rclone.sh` + Forge role monitor |
| 1 | Nomad + Consul control plane | **Done** (T440 server + cesarops2 client) |
| 2 | Pingora ingress | **Done** — routes updated for dual-coder-zaya (redeploy on T440) |
| 3 | Vector observability | **Done** — `vector-agent` system job |
| 4 | rclone state sync | **Done** — verified on T440 (`cluster-verify-rclone.sh` 7/7) |
| 5 | dora-rs pilot | **Done (bridge)** — `cesarops-dora-runner` on T440 |

## Forge layout (important)

| Node | URL | Role |
|------|-----|------|
| **cesarops2** | `http://127.0.0.1:9100` / `http://10.0.0.201:9100` | **PRIMARY** — routing, `/send`, presets |
| T440 | `http://10.0.0.61:9100` | **Deprecated** — run `scripts/t440-disable-deprecated-forge.sh` on T440 |

Preset: **dual-coder-zaya** — thinker ZAYA `:5203`, coders `:5001` + `:5200`, reviewer `:5002`.

See [FORGE_PRIMARY.md](./FORGE_PRIMARY.md).

## Cluster membership

| Node | Nomad name | Class | Consul |
|------|------------|-------|--------|
| T440 | `t440cesarops` | `t440-p100` | server |
| cesarops2 | `cesarops2` | `cesarops2-worker` / `rtx2060` | client (LAN `10.0.0.201` → `10.0.0.61`) |
| nautik9 | `nautik9` | `worker` | left / down |

## Nomad jobs

| Job | Status | Notes |
|-----|--------|-------|
| `forge-api` | **retired** | `count = 0` — do not bind T440 `:9100` |
| `vector-agent` | running | System job |
| `pingora-edge` | running | Redeploy after route change (`:8088`) |
| `dora-pilot` | running | HTTP pilot |
| `rclone-state-sync` | periodic | Every 15m on T440; needs `rclone-setup-t440.sh` once |

## Operator commands

**Nomad (LAN from any node):**

```bash
export NOMAD_ADDR=http://10.0.0.61:4646
nomad job status
nomad job run infra/nomad/jobs/pingora-edge.nomad.hcl
nomad job run infra/nomad/jobs/rclone-state-sync.nomad.hcl
nomad job periodic force rclone-state-sync
```

**rclone (T440 once):**

```bash
bash scripts/rclone-setup-t440.sh
bash infra/nomad/rclone/rclone-state-sync.sh
```

**Forge routing (c2):**

```bash
bash scripts/forge_apply_dual_coder_zaya.sh
sudo systemctl restart cesarops-forge-v2.service
```

**Verify without Forge:**

```bash
bash scripts/cluster-verify-rclone.sh
```

## Remaining work (unification checklist)

| Item | Status |
|------|--------|
| Nomad + Consul + Vector + rclone + dora bridge | Done |
| Forge primary on c2 + fleet-hetero layout | Done |
| **Cross-review pipeline** (Gemma↔Qwen, 1070 race, polisher queue) | **Scripted** — `run_fleet_hetero_pipeline.sh`; not yet in Forge `/send` loop |
| **Dynamic GPU watchdog** (heartbeat → unload → reload per `gpu_uuid`) | Spec only — replace `mission_service_watchdog` |
| **Stable model loads** (stop n8n watchdog timer; RTX E4B + 1070 Qwen on correct ports) | Operator |
| **Script cleanup mission** — [SCRIPT_CLEANUP_FORGE_MISSION.md](./SCRIPT_CLEANUP_FORGE_MISSION.md) | Inventory done; Forge verdict + n8n ephemeral delete pending |
| T440 deprecated Forge `:9100` | Operator |
| nautik9 Nomad client | Down / re-bootstrap |
| Pingora SLO / role health gates | Optional |
| Native dora-rs mission graphs | Future |

1. **Script cleanup (Forge mission)** — inventory done; Phase 2 `forge-verdict.json` + Phase 3 n8n hygiene.
2. **T440** — `t440-disable-deprecated-forge.sh` (keep `:9100` stopped).
3. **Fleet bench** — `bash scripts/role_bench/run_fleet_hetero_pipeline.sh` after `cesarops2_fleet_roles.sh` + `p100_gemma_r1_dual.sh` + polisher.
4. **Watchdog** — implement dynamic restorer from thinker spec.
5. **nautik9** — re-bootstrap or drain.
6. **Full dora-rs** — native graph nodes when ready.

## Related docs

- [FORGE_PRIMARY.md](./FORGE_PRIMARY.md)
- [RCLONE_T440_SETUP.md](./RCLONE_T440_SETUP.md)
- [infra/nomad/README.md](../infra/nomad/README.md)
