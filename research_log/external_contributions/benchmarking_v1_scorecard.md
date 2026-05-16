# Benchmarking + Leaderboard v1 — External Contribution

Source: dropped in by operator from a friend, 2026-05-16. Companion to
the multi-model and MoE drops in this directory.
Status: **REFERENCE MATERIAL — NOT INTEGRATED YET.**

The shape: on every model load, run a micro-benchmark, compare against
a reference leaderboard JSON, log to CSV, expose results to the operator.

## What it ships

1. `src/telemetry/benchmark.rs` — `EngineBenchmarker::execute_on_load_benchmark`
   runs a warmup + 20-iteration timed loop, emits a `BenchmarkResult`
   (tokens/sec, latency, bandwidth GBps), and `compare_and_score` diffs
   against a local `BenchmarkLeaderboard` JSON.

2. `scripts/benchmark_sync.py` — bootstraps the leaderboard JSON,
   triggers the engine binary with `--run-bench`, parses stdout for
   "Current Execution: N tokens/sec", appends to CSV.

3. Integration hook in `src/server/routes.rs` — appends a benchmarking
   step to `handle_load_model` after the VRAM budget check.

This pairs with the multi-model drop (`registry/device.rs`,
`registry/model.rs`) and finishes the "load → measure → log" loop.

## Polish notes for integration

The contributed code as-is would compile after a few edits — the
benchmarking *concept* is right but the implementation is mocked:

1. **`tokens_per_second` is a hardcoded `45.2 + rand()`** — placeholder.
   Real version needs to actually run a forward pass through the
   loaded model and divide tokens / wall time. We have all the pieces
   (`generate_tokens`, `ModelConfig`, `KVCache`); just thread them in.

2. **`memory_bandwidth_gbps` derivation is fake** — `tokens_per_sec * 4.0 * 0.001`
   is meaningless. Real bandwidth comes from
   `bytes_read_per_token / time_per_token`. Per-layer FLOPS + bandwidth
   needs `wgpu::QuerySet` timestamp queries (Pascal Vulkan supports them).

3. **`gpu_name` resolution is a hardcoded if-else.** Replace with
   `adapter.get_info().name` from the device registry.

4. **`queue.submit(None)` warmup loop** — `submit` doesn't take an
   `Option`, it takes an `IntoIterator<Item=CommandBuffer>`. Use
   `queue.submit(std::iter::empty())` or actually submit warmup work.

5. **Stdout-parsing in the Python script is brittle.** Better approach:
   have the engine emit JSON to a `--bench-out` file (or stdout when
   a `--json` flag is set), Python reads JSON.

6. **No `--run-bench` CLI flag** in our `parse_args` today. Need to
   add it.

7. **Adds `rand` crate dep** for the simulated jitter — drop entirely
   once we measure real numbers.

8. **`config/benchmark_leaderboard.json`** path is unconventional; we
   already use `~/.cache/cesarops/` (`telemetry_tuner` + `pipeline_cache`).
   Standardize on the cache dir.

## Why this is a real win, not just a nice-to-have

Today we have ZERO production FLOPS measurement on the engine. The
cached `33.1 GFLOPS` is from a 1×1536×1536 microbenchmark, not real
forward passes. The operator recalled "we were at about 2 TFLOPS"
which I currently can't confirm or refute — that's exactly the gap
this benchmarking work closes.

The instrumentation also unblocks every future optimization spec:
- "Did Q6_K fused matvec actually help?" — needs before/after numbers.
- "Is push-constants saving us anything?" — needs per-kernel timing.
- "Are we GPU-bound or dispatch-bound?" — needs `wgpu::QuerySet`.
- "How much does multi-model contention cost?" — needs per-model t/s.

## Integration order (after multi-model lands)

1. Add `--bench` mode to engine CLI: load model, run N tokens with
   `wgpu::QuerySet` timestamp queries on each pipeline, emit JSON
   with per-kernel timing + total t/s + measured FLOPS.
2. Implement real `EngineBenchmarker` (drop the random jitter).
3. Move leaderboard to `~/.cache/cesarops/leaderboard.json`.
4. Hook into multi-model load lifecycle (the drop's
   `handle_load_model_with_telemetry` shape is right).
5. Smoke-test on T440 P100 + cesarops2 1070, get real numbers,
   update `DEBUG_LOG.md` with measured tps + FLOPS.
6. Optional: Python `scripts/benchmark_sync.py` wired to a remote
   reference list — but skip until we have real local numbers worth
   comparing.

## What this unlocks

Once this lands we can answer the operator's "what TFLOPS are we
actually doing?" question authoritatively, every load. It also gives
the forge a real model scorecard signal (currently scorecard tracks
only attempt success/failure rate, not throughput).
