# Which Forge is authoritative?

## Primary (use this)

| Host | URL | Role |
|------|-----|------|
| **cesarops2** | `http://127.0.0.1:9100` (LAN `http://10.0.0.201:9100`) | **Active Forge** — dual-coder + ZAYA routing, punch list, Nomad client |

On cesarops2 always:

```bash
export FORGE_URL=http://127.0.0.1:9100
source /mnt/t440/codebase/repos/wreckhunter2000-1/scripts/cesarops2-isolated.env
bash scripts/forge_apply_dual_coder_zaya.sh
```

## Removed on T440

Forge is **not** on T440 (`:9100` free). Do **not** use `http://10.0.0.61:9100` for Forge API.

From **cesarops2** (no SSH to T440):

```bash
export NOMAD_ADDR=http://10.0.0.61:4646
bash scripts/cluster-exec-t440.sh t440-remove-forge
# or fleet: FLEET_DISPATCH=nfs bash scripts/fleet-n8n-dispatch.sh cesarops2 t440_remove_forge
```

Nomad runs the remove script on the `t440-p100` node. T440-only LLMs (`:5001`, `:5002`) stay up; only Forge/systemd on `:9100` is removed.

Nomad `forge-api` on T440 is a **placeholder** (`sleep infinity`); it is not the real Forge binary.

## Shared NFS state

Both nodes mount the same repo tree. Only **cesarops2** should own:

- `cesarops-forge-v2/routing_state.json`
- `cesarops-forge-v2/mode_state.json`

The user timer `forge-sync-state.timer` on c2 restores snapshots from `routing/*dual-coder-zaya.json` every 45s.

## Scripts that must stay on c2

- `scripts/forge_apply_dual_coder_zaya.sh`
- `scripts/forge_role_monitor.sh` (with `FORGE_URL=http://127.0.0.1:9100`)
- `scripts/forge-routing-switch.sh` (no T440 API fallback when `CESAROPS2_ISOLATED=1`)

## Failover (read-only)

If c2 Forge is down, you may **read** T440 health for diagnosis — do not apply presets there unless intentionally reverting to T440 conductor mode (`forge-routing-switch.sh backup`).
