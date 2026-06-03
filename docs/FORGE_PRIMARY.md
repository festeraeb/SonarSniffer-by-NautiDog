# Which Forge is authoritative?

## Unified fleet (current)

When `~/.cache/cesarops/fleet-unified` exists (`bash scripts/fleet unified up`):

| Host | URL | Role |
|------|-----|------|
| **T440** | `http://10.0.0.61:9100` (local on T440: `http://127.0.0.1:9100`) | **Forge primary** — dual-coder routing to P100 `:5001`/`:5002` |
| **cesarops2** | Use **LAN** `http://10.0.0.61:9100` | Operator node; repo + jobs via NFS (`/mnt/t440/repo`) |

```bash
source scripts/lib/fleet_resolve.sh   # sets FORGE_URL=http://10.0.0.61:9100 when unified
bash scripts/fleet status
curl -s http://10.0.0.61:9100/forge/status | jq .
```

Config: `config/fleet_manifest.json` → `unified.forge_url`.

## Legacy (pre-unified / isolated c2)

| Host | URL | Role |
|------|-----|------|
| **cesarops2** | `http://127.0.0.1:9100` | Forge on c2 only |

```bash
export FORGE_URL=http://127.0.0.1:9100
bash scripts/forge_apply_dual_coder_zaya.sh
```

## Do not run two Forges

If both `127.0.0.1:9100` (c2) and `10.0.0.61:9100` (T440) answer `/health`, stop the legacy c2 unit:

```bash
systemctl --user stop cesarops-forge-v2.service 2>/dev/null || true
```

Unified operators should **only** call T440 `:9100`.

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
