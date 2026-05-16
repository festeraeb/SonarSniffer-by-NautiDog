# Benchmark + Scorecard Telemetry — External Contribution

Source: dropped in by operator, 2026-05-16. Companion to the multi-model
and MoE drops in this directory.
Status: **REFERENCE MATERIAL — NOT INTEGRATED YET.**

This adds a load-time benchmark + global leaderboard comparison pattern.
Sketch of what's contributed:

1. `src/telemetry/benchmark.rs` — `EngineBenchmarker` runs a quick
   warm-up + N-iteration timed loop on model load, produces a
   `BenchmarkResult { tokens_per_sec, latency_ms, memory_bandwidth_gbps }`,
   compares against a `BenchmarkLeaderboard` of historical scores.
2. `scripts/benchmark_sync.py` — fetches the leaderboard JSON,
   triggers the engine binary with `--run-bench`, parses console output,
   appends to `logs/engine_benchmarks.csv`.
3. Lifecycle hook into `handle_load_model` so every model load runs
   the benchmark and prints a scorecard.

Important caveats in the current draft:
- The benchmark does `queue.submit(None)` in a loop, then derives a
  `simulated_tokens_per_sec = 45.2 + rand()` value. That's not an
  actual measurement — it's a placeholder. Real version needs to
  run an actual forward pass against the loaded model.
- Memory-bandwidth math is `tokens_per_sec * 4.0 * 0.001` which
  isn't a meaningful bandwidth figure.
- GPU name is hardcoded based on `gpu_index` ("Tesla P100" or
  "GTX 1080 Ti"). Real version pulls from `wgpu::AdapterInfo`.
- `rand` crate isn't yet a dep; same flag we hit on the MoE pool.
- `subprocess.Popen` parsing in the Python script is fragile —
  better to have the engine emit a JSON line on stdout that the
  script parses with `json.loads(output)`.

The pattern is right (load-time micro-bench + leaderboard), it just
needs to actually measure something. When we wire this up properly
it'll replace the cached 33 GFLOPS we see today with real per-model
TFLOPS numbers.
