# Cake (upstream) vs cake_kv (cesarops-inference)

| Name | Location | Purpose |
|------|----------|---------|
| **Cake** | `/opt/cesarops/cake/bin/cake` (`cake-cli` from crates.io) | Fleet distributed inference for **idle audit** (`cake_fleet` mode) |
| **cake_kv** | `cesarops-inference/src/cake_kv.rs` | Future native engine KV paging — **not** used by fleet-mode scripts |

Install: [`docs/CAKE_INSTALL.md`](CAKE_INSTALL.md). Fleet scripts: [`scripts/cesarops-fleet-mode.sh`](../scripts/cesarops-fleet-mode.sh), [`scripts/cake/start-fleet-cluster.sh`](../scripts/cake/start-fleet-cluster.sh), topologies under [`scripts/cake/`](../scripts/cake/).
