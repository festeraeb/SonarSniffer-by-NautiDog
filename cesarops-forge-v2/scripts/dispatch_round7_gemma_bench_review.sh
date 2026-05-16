#!/bin/bash
# Dispatch to Gemma-4-MoE on P100 #0 (port 5001).
# Task: review the contributor's benchmarking design and surface any
# real correctness issues we should know about before we polish it.
# This is the "second eyes" pass — we already know placeholder tokens/sec
# is fake. Looking for things like measurement methodology errors,
# wgpu API misuse, or design flaws.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

cat > "$OUT_DIR/r7_gemma_bench_review.prompt" << 'PROMPT_EOF'
You are a Rust + wgpu expert reviewing benchmarking code for an inference engine on Pascal P100. Spot real correctness issues. Output a numbered list of issues, sorted by severity. NO commentary. NO markdown fences. NO <think> blocks.

CODE TO REVIEW:

```rust
pub async fn execute_on_load_benchmark(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    model_id: &str,
    gpu_index: usize,
) -> BenchmarkResult {
    let gpu_name = if gpu_index == 0 { "NVIDIA Tesla P100" } else { "NVIDIA GTX 1080 Ti" };
    println!("Launching micro-benchmark for {} on {}...", model_id, gpu_name);

    // Warmup
    for _ in 0..3 {
        device.poll(wgpu::Maintain::Wait);
    }

    let iterations = 20;
    let start = Instant::now();
    for _ in 0..iterations {
        queue.submit(None);
    }
    device.poll(wgpu::Maintain::Wait);
    let total_duration = start.elapsed();
    let avg_latency = total_duration.as_secs_f32() / (iterations as f32);

    let simulated_tokens_per_sec = 45.2 + (rand::random::<f32>() * 3.0);
    let calculated_gbps = (simulated_tokens_per_sec * 4.0 * 0.001);

    BenchmarkResult {
        model_id: model_id.to_string(),
        gpu_name: gpu_name.to_string(),
        tokens_per_second: simulated_tokens_per_sec,
        latency_ms: avg_latency * 1000.0,
        memory_bandwidth_gbps: calculated_gbps,
    }
}
```

Skip the obvious issues (hardcoded gpu_name, simulated tokens_per_sec, fake bandwidth formula, fake latency from empty submits). I already see those.

Find the LESS obvious issues. Examples of what to look for:
- wgpu API misuse (e.g., does `queue.submit(None)` even compile in wgpu 24?)
- Async/poll race conditions
- Missing fences/barriers between iterations
- Pascal-specific gotchas
- Methodology errors that would persist even after the fake numbers are replaced

If nothing's wrong beyond the obvious, say so in one line.
PROMPT_EOF

PAYLOAD=$(python3 -c "import json; p=open('$OUT_DIR/r7_gemma_bench_review.prompt').read(); print(json.dumps({'prompt': p, 'max_length': 1500, 'temperature': 0.2, 'top_p': 0.9, 'rep_pen': 1.05}))")

curl -s -m 600 -X POST http://127.0.0.1:5001/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  > "$OUT_DIR/r7_gemma_bench_review.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r7_gemma_bench_review.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r7_gemma_bench_review.txt" 2>"$OUT_DIR/r7_gemma_bench_review.err"
echo "[Gemma-4 r7] bench_review: $(wc -l < "$OUT_DIR/r7_gemma_bench_review.txt") lines"
