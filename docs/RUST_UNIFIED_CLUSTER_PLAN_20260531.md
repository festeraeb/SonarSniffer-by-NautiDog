# Rust-first unified node plan (Pingora + Nomad + Vector + rclone + dora-rs)

Date: 2026-05-31

## Goal

Unify T440, laptop, and future nodes under one lightweight control plane that is:

- Rust-first for data plane and workflow orchestration
- Not Docker Swarm
- Friendly to mixed GPU/CPU nodes and intermittent links
- Observable and easy to recover

## Executive architecture

1. Scheduling/control plane: Nomad + Consul
2. Service ingress/routing: Pingora (Rust) with service discovery from Consul
3. Logs/metrics pipeline: Vector agents on every node + central Vector aggregator
4. Data mobility and backup: rclone jobs (Nomad batch/system jobs)
5. Workflow graph runtime: dora-rs for mission and sensor pipelines

This keeps orchestration simple and proven (Nomad/Consul), while using Rust-native tools for runtime traffic and dataflow (Pingora/dora-rs/Vector where possible).

## Why this fits your stack

- You already run heterogeneous nodes with mixed uptime; Nomad handles that better than static scripts.
- You want larger specialist models on P100s; Nomad constraints can pin roles to exact GPU nodes.
- You already have webhook/fleet behavior; Pingora can front these services with health-aware routing.
- You do not want Swarm; this avoids Swarm entirely.

## Node roles

### T440 (primary)

- Nomad server + client
- Consul server + client
- Pingora edge/router
- Vector aggregator + local agent
- dora-rs runtime for core orchestration graphs
- Primary rclone remote sync origin

### cesarops2 / laptop / future nodes

- Nomad client
- Consul client
- Vector agent
- Optional Pingora sidecar (only if node-local edge is needed)
- dora-rs worker nodes for specialized pipelines
- rclone local cache/sync jobs

## Service classes

1. LLM inference services
- `llm-coder` (P100 lane)
- `llm-reviewer-science` (P100 lane)
- `llm-thinker-tools` (RTX or best-available lane)

2. Ops/control services
- `forge-api`
- `n8n-webhook`
- `fleet-job-runner`

3. Data pipeline services
- `sat-mission`
- `mag-mission`
- `detection-orchestrator`

## Scheduling model (Nomad)

Use Nomad job specs with constraints and metadata:

- Node meta examples:
  - `node.class = t440-p100`
  - `gpu.class = p100|rtx2060|m2200`
  - `zone = lan|tailscale`
- Job constraints:
  - Coder/reviewer jobs constrained to `gpu.class = p100`
  - Thinker/tool planner constrained to `gpu.class = rtx2060` (or fallback class list)

Use job types:

- `service` for always-on APIs/inference
- `batch` for missions, reindexes, backups, long jobs
- `system` for Vector and host-level collectors

## Pingora design

Pingora acts as one smart ingress for Forge + internal services:

- Upstreams from Consul service catalog
- Health checks and circuit breaking per upstream
- Weighted routing for role-specific traffic
- Request tags for role (`coder/reviewer/thinker`) and task class
- Retry policy only for idempotent endpoints

Suggested routes:

- `/api/forge/*` -> Forge pool
- `/api/llm/coder` -> coder service group
- `/api/llm/reviewer` -> reviewer service group
- `/api/llm/thinker` -> thinker service group
- `/api/fleet/*` -> n8n + fleet runner

## Vector design

Per-node Vector agent:

- Inputs: journald, file logs, app stdout, optional metrics source
- Transforms: normalize node/service/role labels
- Sinks: central Vector aggregator on T440 + optional archival sink

Central Vector aggregator:

- Route to local object/file store and searchable backend
- Build dashboards for:
  - Model load time
  - Endpoint 5xx/timeout rates
  - Role success/fail counters
  - Job duration and retry counts

## rclone strategy

Use rclone for deterministic state sync, not ad-hoc copy scripts:

- Sync classes:
  - Config snapshots (`cluster_config.toml`, state files, Nomad jobs)
  - Run artifacts (`/tmp` summaries promoted to persistent path)
  - Model metadata/index manifests (not full model copies on each cycle)
  - Logs cold storage

- Recommended job cadence:
  - Fast: every 5-10 min for state/artifacts
  - Slow: nightly for cold logs

- Safety:
  - `--immutable` for archived artifacts
  - checksums and dry-run verification in batch jobs

## dora-rs role

Use dora-rs as the Rust dataflow runtime for mission pipelines and tool chains:

- Convert high-value orchestrator paths into dora graphs:
  - mission intake -> enrich -> inference -> validate -> publish
- Keep n8n for webhook/operator UX initially
- Over time, move critical execution semantics to dora-rs nodes for stronger typing and replayability

## Security model

- Tailscale for node-to-node transport
- Consul ACLs enabled
- Nomad ACLs + namespace separation (`prod`, `exp`, `ops`)
- rclone remotes with encrypted config and least privilege
- Pingora mTLS (internal) or service token headers validated at edge

## Phased implementation plan

## Phase 0: Baseline freeze (1-2 days)

Deliverables:

- Inventory of current services/endpoints/jobs
- Canonical node metadata (`node.class`, `gpu.class`, labels)
- Current SLO baseline (latency, error rate, load times)

Exit criteria:

- Reproducible baseline report for 3 core role paths

## Phase 1: Control plane bootstrap (2-3 days)

Deliverables:

- Nomad + Consul on T440, clients on other nodes
- Service registrations for Forge, inference endpoints, n8n
- Minimal job specs for existing services

Exit criteria:

- All critical services discoverable via Consul
- One click/job deploy from Nomad for at least one inference endpoint

## Phase 2: Pingora ingress (2-4 days)

Deliverables:

- Pingora service with role-based routes
- Health checks + failover policy
- Traffic/latency metrics emitted to Vector

Exit criteria:

- Role traffic no longer hardcoded to raw host:port in client paths
- Controlled failover works in test

## Phase 3: Observability with Vector (1-2 days)

Deliverables:

- Vector agents on all nodes
- Central aggregation and labels normalized
- Dashboards/alerts for timeout and model-load regressions

Exit criteria:

- You can identify failing node/role in under 2 minutes from logs/metrics

## Phase 4: rclone state and artifact sync (1-2 days)

Deliverables:

- Nomad batch jobs for periodic sync
- Artifact retention policy
- Restore runbook tested

Exit criteria:

- Recovery from node loss validated with latest state and artifacts

## Phase 5: dora-rs pilot graph (3-5 days)

Deliverables:

- One production-relevant pipeline represented as dora graph
- Bridge adaptor with current Forge/n8n calls
- Success/failure replay path

Exit criteria:

- Pilot graph runs end-to-end with measurable reliability gain

## Suggested first implementation slice (lowest risk)

1. Nomad + Consul bootstrap
2. Pingora in front of current Forge + role endpoints
3. Vector node agents + central sink
4. rclone sync jobs for state + artifacts
5. dora-rs only for one mission subgraph

This gives immediate unification without forcing a full rewrite.

## Rust-first tooling notes

- Pingora: Rust edge/data plane, high-performance, good fit for role-routing
- Vector: Rust, mature, excellent for fleet observability
- dora-rs: Rust-native graph runtime for typed orchestration evolution
- Nomad: not Rust, but pragmatic and lightweight scheduler for heterogeneous fleet
- rclone: not Rust, but highly reliable for sync/backup workflows

## Explicit non-goals (for now)

- No Docker Swarm
- No full migration away from existing n8n on day one
- No immediate rewrite of all Python/bash orchestration

## Risks and mitigations

1. Preset/state drift across nodes
- Mitigation: make Nomad jobs and routing state authoritative; sync with rclone on schedule

2. Mixed endpoint readiness and model load delays
- Mitigation: Pingora health gates + warmup checks before routing activation

3. Cross-node config skew
- Mitigation: immutable versioned config bundles + rollout promotion gates

## Acceptance checklist

- Three specialist model lanes are routable by role through Pingora
- Nomad can redeploy all core services without manual ssh choreography
- Vector shows per-role health and timeout trends
- rclone restores latest known-good routing and run artifacts
- One dora-rs mission graph runs in production shadow mode
