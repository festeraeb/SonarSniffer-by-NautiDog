# Nomad Bare-Metal Rust-First Bootstrap

This folder contains starter assets to execute the Rust-first unified node plan.
It covers the full listed toolchain scaffold: Nomad, Consul, Pingora, Vector, rclone, and dora-rs.

Scope for this bootstrap:
- Nomad + Consul control plane on T440.
- Nomad client join on secondary nodes.
- Service jobs for Forge and observability primitives.
- Vector node-agent job and a sample rclone batch job.
- Staged Pingora edge job and staged dora-rs pilot graph job.

Out of scope for this slice:
- Full Forge remediation.
- Full dora-rs migration.

## Suggested order

1. Configure and start Consul server/client on T440.
2. Configure and start Nomad server/client on T440.
3. Configure and start Consul + Nomad clients on each secondary node.
4. Submit jobs from `jobs/`.
5. Validate service discovery and allocations.

## Environment assumptions

- **LAN fleet (always co-located):** T440 `10.0.0.61`, cesarops2 `10.0.0.201` — Consul/Nomad use these; no Tailscale required between them.
- **Laptop (optional remote):** `bootstrap_client.sh <class> <gpu> --remote` joins via Tailscale (`CESAROPS_T440_TS_IP`, default `100.72.182.77`). Set `CESAROPS_CONSUL_ADVERTISE=$(tailscale ip -4)` so Nomad does not advertise WSL `172.x`. Cloudflare tunnel is for HTTP ingress (Forge/UI), not cluster gossip.
- Datacenter: `dc1`
- Node meta includes `node.class`, `gpu.class`, and `zone`

## Quick validation

- `consul members`
- `nomad server members`
- `nomad node status`
- `nomad job run infra/nomad/jobs/forge-api.nomad.hcl`
- `nomad status forge-api`
