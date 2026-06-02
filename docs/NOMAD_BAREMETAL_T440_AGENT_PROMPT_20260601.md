# T440 Agent Prompt: Nomad Bare-Metal Rust Conversion (Phase 1)

Use this as the exact execution brief for the T440 agent.

## Objective

Implement the full stack scaffold from the Rust-first unified roadmap on T440:
- Bootstrap Consul + Nomad control plane.
- Register Forge as a Nomad-managed service.
- Stand up baseline Vector jobs.
- Stage Pingora edge routing and dora-rs pilot graph jobs.
- Stage rclone state sync job.
- Keep current Forge behavior intact (no deep Forge fixes yet).

## T440 Path Contract

- Expected repo root on T440: `/codebase/wreckhunter2000-1`
- If the preflight reports a different valid mount path, use that exact path consistently for all commands.

## Mandatory Preflight

Before any action, run:

```bash
/codebase/wreckhunter2000-1/infra/nomad/scripts/require_mounted_repo.sh /codebase/wreckhunter2000-1
```

If it fails, stop and report.

## Repo Assets To Use

- /codebase/wreckhunter2000-1/infra/nomad/consul/server.hcl
- /codebase/wreckhunter2000-1/infra/nomad/consul/client.hcl
- /codebase/wreckhunter2000-1/infra/nomad/nomad/server.hcl
- /codebase/wreckhunter2000-1/infra/nomad/nomad/client.hcl
- /codebase/wreckhunter2000-1/infra/nomad/jobs/forge-api.nomad.hcl
- /codebase/wreckhunter2000-1/infra/nomad/jobs/vector-agent.nomad.hcl
- /codebase/wreckhunter2000-1/infra/nomad/jobs/pingora-edge.nomad.hcl
- /codebase/wreckhunter2000-1/infra/nomad/jobs/dora-pilot.nomad.hcl
- /codebase/wreckhunter2000-1/infra/nomad/jobs/rclone-state-sync.nomad.hcl
- /codebase/wreckhunter2000-1/infra/nomad/vector/aggregator.toml
- /codebase/wreckhunter2000-1/infra/nomad/rclone/rclone-state-sync.sh
- /codebase/wreckhunter2000-1/infra/nomad/dora/mission_pilot_graph.yaml
- /codebase/wreckhunter2000-1/docs/RUST_UNIFIED_CLUSTER_PLAN_20260531.md

## Guardrails

- Do not migrate everything at once.
- Do not remove existing systemd services until Nomad service health is proven.
- Keep all changes idempotent and scriptable.

## Task Checklist

1. Install prerequisites on T440:
- nomad
- consul
- vector
- rclone

2. Place config files:
- Copy `/codebase/wreckhunter2000-1/infra/nomad/consul/server.hcl` -> `/etc/consul.d/consul.hcl`
- Copy `/codebase/wreckhunter2000-1/infra/nomad/nomad/server.hcl` -> `/etc/nomad.d/nomad.hcl`

3. Start services:
- `systemctl enable --now consul`
- `systemctl enable --now nomad`

4. Validate cluster baseline:
- `consul members`
- `nomad server members`
- `nomad node status`

5. Run initial jobs:
- `nomad job run /codebase/wreckhunter2000-1/infra/nomad/jobs/forge-api.nomad.hcl`
- `nomad job run /codebase/wreckhunter2000-1/infra/nomad/jobs/vector-agent.nomad.hcl`

6. Stage or run extended jobs:
- `nomad job run /codebase/wreckhunter2000-1/infra/nomad/jobs/pingora-edge.nomad.hcl`
- `nomad job run /codebase/wreckhunter2000-1/infra/nomad/jobs/dora-pilot.nomad.hcl`
- `nomad job run /codebase/wreckhunter2000-1/infra/nomad/jobs/rclone-state-sync.nomad.hcl`

If Pingora or dora binaries are missing, leave those jobs staged and report the exact missing binary path.

7. Confirm service health:
- `nomad status forge-api`
- `nomad alloc status <alloc_id>`
- `curl http://127.0.0.1:9100/health`

8. Keep rclone batch optional if remote not configured:
- verify remote with `rclone listremotes`
- only enable recurring sync if `cesarops-backup` exists

## Required Output Back To Operator

Return a concise report with:
- Commands run
- Exit status per command
- Nomad job IDs and allocation IDs
- Any blocking errors
- Exact file edits or overrides made

## If Blocked

If installation or systemd unit naming differs on the host:
- Detect actual unit names and package paths.
- Continue with equivalent commands.
- Document deviations in final report.
