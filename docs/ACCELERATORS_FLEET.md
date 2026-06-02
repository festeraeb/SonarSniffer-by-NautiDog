# Accelerators — TPU (c2) + Movidius (T440)

## Hardware map (verified 2026-06-01)

| Device | Node | Bus | Fleet role |
|--------|------|-----|------------|
| **Coral Edge TPU** PCIe `1ac1:089a` | cesarops2 | `07:00.0` | `POST http://10.0.0.201:8092/infer` |
| **Intel Movidius NCS** USB `03e7:2150` | **T440** (not c2) | USB | `POST http://10.0.0.61:8180/jitter` |

Docs that said Movidius on cesarops2 USB were wrong; Nomad probe on T440 shows the stick on T440.

## Fleet endpoints

| URL | Service | Status |
|-----|---------|--------|
| `http://10.0.0.201:8092/health` | Edge TPU gateway (`scripts/start_edgetpu_server_c2.sh`) | Up (CPU stub until gasket driver loads) |
| `http://10.0.0.61:8180/health` | Lock-3 **`jitter-rs`** (`t440-movidius-jitter` Nomad job) | Deploy via Nomad |
| `http://10.0.0.201:8190/health` | Coral jitter validator (ML350e; optional remote vote) | `start_coral_jitter_worker.sh` |
| `http://127.0.0.1:5580/health` | cesarops-detection (uses `JITTER_URLS` pool) | Up on c2 |

Config pools in `cesarops-forge-v2/cluster_config.toml`:

- `[endpoint_pool.accelerator_tpu]`
- `[endpoint_pool.accelerator_jitter]`

Detection default jitter list includes `http://10.0.0.61:8180` first (`cesarops-detection/src/endpoint_pool.rs`).

## Operators

```bash
# Probe all accelerators + HTTP endpoints
bash scripts/accelerator_fleet_probe.sh

# Forge (after rebuild): GET /cluster/accelerators

# c2 — TPU server (stub until /dev/apex_* exists)
bash scripts/start_edgetpu_server_c2.sh

# T440 — Lock-3 jitter-rs (Nomad from c2)
export NOMAD_ADDR=http://10.0.0.61:4646
nomad job run infra/nomad/jobs/t440-movidius-jitter.nomad.hcl

# ML350e — Coral validator for jitter-rs remote vote (Python HTTP sidecar)
bash cesarops-detection/jitter-rs/scripts/start_coral_jitter_worker.sh
```

## Lock-3 stack (Rust-first)

| Layer | Implementation |
|-------|----------------|
| Primary `:8180` | **`jitter-rs`** on T440 (tract CPU / heuristic; same HTTP contract as legacy Python) |
| Movidius vote | `jitter-rs --features movidius` (detect-only until OpenVINO 2022.3 + IR model) |
| Coral vote | Remote `http://10.0.0.201:8190` via `JITTER_REMOTE_VALIDATORS` |

Legacy `jitter_movidius.py` is deprecated; do not start it on `:8180` alongside `jitter-rs`.

## TPU driver gap (c2)

PCI Coral is visible but **`/dev/apex_*` missing** — `gasket-dkms` failed to build on kernel `6.8.0-117-generic`. `libedgetpu1-std` is installed. Until gasket loads, TPU inference is CPU stub (still exposes `/infer` + `/health` for fleet wiring).

Fix path: repair DKMS build (`/var/lib/dkms/gasket/1.0/build/make.log`) or use a supported kernel / USB Coral Accelerator.

## Movidius / OpenVINO

Real NCS2 inference needs OpenVINO **2022.3** + IR model (`OPENVINO_2022_RUNTIME`). Current fleet OpenVINO has no `MYRIAD` device — `jitter-rs` uses CPU primary + Movidius as validator metadata until that runtime is wired.

## cesarops-node

Heartbeat reports USB `03e7` and `/dev/apex_*` to Forge when `cesarops-node` runs on each host.
