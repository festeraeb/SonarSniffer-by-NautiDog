# jitter-rs — Lock-3 Jitter/Thermal Analyst (Rust)

Pure-Rust replacement for the legacy Python `jitter_movidius.py` worker.

## Why this exists

The old worker set `OPENVINO_DEVICE=MYRIAD` and `pip install openvino`, but Intel
removed the MYRIAD plugin in OpenVINO 2023.0. On the current fleet (OpenVINO
2026.x, Ubuntu 24.04) that path **silently fell back to the CPU heuristic** — the
Movidius stick was never actually inferring.

This crate makes the design explicit and durable:

- **Primary inference**: pure-Rust [`tract`](https://crates.io/crates/tract-onnx)
  on CPU (no C/C++ deps). GPU path reserved behind `--features gpu`.
- **Cross-validation in the same flow**: heterogeneous accelerators vote on the
  primary candidate. Agreement raises certainty; disagreement lowers it (and can
  flip a weak `ferrous_composite` back to `natural`).
  - `movidius` — Intel NCS2 / Myriad X (USB `03e7:2150`), present on the T440.
  - `edgetpu` — Coral Edge TPU (`libedgetpu.so`), present on the ML350e.
- **Graceful degradation**: with no model it runs the deterministic thermal
  heuristic (identical contract to the Python worker). Validators are
  feature-gated, so the default build runs anywhere.

## HTTP contract (unchanged from Python)

```
GET  /health  -> { service, primary_backend, validators[], status }
POST /jitter  -> JitterSignature
```

`JitterSignature` adds two fields over the legacy shape:
- `validation[]` — per-device votes (`device`, `agreement`, `agreed`, `backend`)
- `primary_backend` — which backend produced the candidate

## Build

```bash
cargo build --release                      # CPU heuristic/tract, no validators
cargo build --release --features movidius  # + NCS2 validator (T440)
cargo build --release --features edgetpu   # + Coral validator (ML350e)
cargo build --release --features gpu       # GPU primary path
```

## Run

```bash
JITTER_PORT=8180 JITTER_MODEL=/path/to/jitter.onnx ./target/release/jitter-rs
```

Env:
- `JITTER_PORT` (default 8180)
- `JITTER_MODEL` — optional ONNX path; absent => heuristic
- `RUST_LOG` — e.g. `info`, `debug`

## Notes on the accelerator validators

The validator `vote()` methods currently derive an independent estimate from the
tile's band/geometry signal so the consensus wiring is exercised end-to-end.
Wiring real model execution:

- **Movidius**: needs an OpenVINO **2022.3** runtime (last MYRIAD-capable
  release). Run inside a `ubuntu:22.04` container with USB passthrough
  (`--device=/dev/bus/usb`) and the Myriad udev rules; set
  `OPENVINO_2022_RUNTIME` to its install dir.
- **Edge TPU (local FFI)**: link `libedgetpu.so` and load an int8 `.tflite`
  compiled with the Edge TPU compiler. Detection reuses the FFI pattern in
  `sovereign-cloud/src/tpu.rs`. Build with `--features edgetpu`.
- **Edge TPU (remote worker)**: the recommended path when the Coral is on
  another node (the ML350e). Run the worker per `CORAL_TPU_WORKER_SPEC.md` and
  point jitter-rs at it:

  ```bash
  JITTER_REMOTE_VALIDATORS="coral_edgetpu=http://<ml350e-ip>:8190" ./jitter-rs
  ```

  Multiple remotes are comma-separated (`device=url,device2=url2`). Each is
  health-probed at startup; unreachable remotes are skipped. Votes use a 750 ms
  timeout and degrade silently on failure. An optional `FLEET_KEY` env adds an
  `X-Fleet-Key` header to remote calls.

  Production worker: `coral_jitter_worker.py` + `scripts/start_coral_jitter_worker.sh`
  (ML350e). Mock: `mock_coral_worker.py` implements the spec for
  integration testing before the real TPU model lands.

Both Movidius and Coral are EOL/aging hardware; the primary pure-Rust CPU/GPU
path is the long-term inference route, with the accelerators acting purely as
corroborating validators.
