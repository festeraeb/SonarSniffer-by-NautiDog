# Benchmark v1 — On-Load Telemetry & Leaderboard

Source: dropped in by operator from a friend, 2026-05-16.
Status: **REFERENCE MATERIAL — NOT INTEGRATED YET.**

This drop adds an automated benchmarking suite that fires on every model
load, captures throughput numbers, compares against a versioned reference
leaderboard, and persists results. Slots cleanly into the multi-model
loading work (extends `handle_load_model` from drop 3).

---

## What it ships

3 pieces:

1. `src/telemetry/benchmark.rs` — `EngineBenchmarker::execute_on_load_benchmark`
   runs warmup + 20-iter timing loop, returns `BenchmarkResult { tokens_per_second,
   latency_ms, memory_bandwidth_gbps }`. Companion `compare_and_score` walks a
   `BenchmarkLeaderboard` (HashMap of model_id → Vec<reference scores>) and
   prints percentage deltas.

2. `scripts/benchmark_sync.py` — pulls reference scores into
   `config/benchmark_leaderboard.json`, kicks the compiled engine via
   `cargo run --release -- serve --model X --gpu N --run-bench`, parses
   stdout, appends to `logs/engine_benchmarks.csv`.

3. `handle_load_model_with_telemetry` HTTP handler — replaces drop-3's
   `handle_load_model` with one that runs the benchmark after VRAM
   verification and returns the scores in the response body.
