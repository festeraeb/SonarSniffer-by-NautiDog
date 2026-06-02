# Laptop Agent Brief (Remote + Local)

Date: 2026-06-01

## Objective

Run as the portable operator agent for the unified rollout across Nomad, Consul, Pingora, Vector, rclone, and dora-rs.

Mounted-drive requirement:
- Before any rollout command, run:
   /codebase/wreckhunter2000-1/infra/nomad/scripts/require_mounted_repo.sh /codebase/wreckhunter2000-1
- If this check fails, stop immediately and report the mount mismatch.

This brief supports two modes:
- Remote mode: laptop controls another node (for example T440) over SSH.
- Local mode: laptop runs control-plane/client steps directly on itself.

## Repo Assets

Primary plan:
- docs/RUST_UNIFIED_CLUSTER_PLAN_20260531.md

Operator brief (T440 focused):
- docs/NOMAD_BAREMETAL_T440_AGENT_PROMPT_20260601.md

Infra scaffold:
- infra/nomad/README.md
- infra/nomad/consul/server.hcl
- infra/nomad/consul/client.hcl
- infra/nomad/nomad/server.hcl
- infra/nomad/nomad/client.hcl
- infra/nomad/jobs/forge-api.nomad.hcl
- infra/nomad/jobs/vector-agent.nomad.hcl
- infra/nomad/jobs/pingora-edge.nomad.hcl
- infra/nomad/jobs/rclone-state-sync.nomad.hcl
- infra/nomad/jobs/dora-pilot.nomad.hcl
- infra/nomad/vector/aggregator.toml
- infra/nomad/rclone/rclone-state-sync.sh
- infra/nomad/dora/mission_pilot_graph.yaml
- infra/nomad/scripts/bootstrap_t440.sh
- infra/nomad/scripts/bootstrap_client.sh

## Mode A: Remote Operator (Laptop -> T440)

Use this mode when the laptop is orchestrating T440 rollout from SSH.

### Remote baseline sequence

1. Sync latest repo to T440 (or pull on T440).
2. Validate target files exist under infra/nomad.
3. Run bootstrap on T440:
   /codebase/wreckhunter2000-1/infra/nomad/scripts/bootstrap_t440.sh
4. Verify:
   - consul members
   - nomad server members
   - nomad node status
   - nomad status forge-api
   - nomad status vector-agent
5. Stage extended jobs:
   - nomad job run infra/nomad/jobs/pingora-edge.nomad.hcl
   - nomad job run infra/nomad/jobs/dora-pilot.nomad.hcl
   - nomad job run infra/nomad/jobs/rclone-state-sync.nomad.hcl (only if remote configured)

### Remote report format

Return exactly:
- Hostname
- Commands executed
- Exit code for each command
- Nomad job IDs
- Allocation IDs
- Blocking error (if any)
- Next safe action

## Mode B: Local Execution (Laptop as Node)

Use this mode when laptop should join cluster as a Nomad client.

### Local client onboarding

1. Install packages:
   - nomad
   - consul
   - vector
2. Run client bootstrap with node metadata:
   /codebase/wreckhunter2000-1/infra/nomad/scripts/bootstrap_client.sh laptop-worker rtx2060
3. Verify local node appears:
   - consul members
   - nomad node status
4. Validate constraints and metadata are correct for scheduling.

### Local safety guardrails

- Do not overwrite T440 server config from laptop.
- Do not run forge-api job on laptop unless explicitly requested.
- Keep laptop role as client worker by default.

## Pingora and dora-rs staging rules

- If binary for pingora-edge is missing, mark job as staged pending build artifact.
- If binary for dora runtime is missing, mark job as staged pending build artifact.
- Do not fake success. Report exact missing path and expected binary name.

## rclone staging rules

- Check remotes first using rclone listremotes.
- Only activate scheduled sync if cesarops-backup exists.
- If absent, report blocked by remote config and keep job staged.

## Health acceptance

Minimum success gate for this phase:
- Consul and Nomad active
- Node membership healthy
- forge-api job running and healthy
- vector-agent job running
- Extended jobs either running or explicitly staged with reason

## Escalation

Escalate immediately if any of these occur:
- nomad server members empty after bootstrap
- consul members shows no leader
- forge-api repeatedly fails allocations
- unexpected config path differences from this repo

When escalating, include exact command output and timestamps.
