#!/bin/bash
# Dispatch to Qwen2.5-Coder-14B (P100 #1, port 5001)
# Task: wgpu pipeline cache module
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

PROMPT='You are a Rust expert. Write a complete, working Rust module pipeline_cache.rs for the cesarops-inference engine that caches wgpu ComputePipeline objects in-memory keyed by (shader_hash, entry_point) and persists shader source files to disk for fast cold-start.

API:
  pub struct PipelineCache { dir: PathBuf, hits: AtomicUsize, misses: AtomicUsize, map: Mutex<HashMap<(u64,String), Arc<wgpu::ComputePipeline>>> }
  impl PipelineCache {
      pub fn new(cache_dir: impl Into<PathBuf>) -> std::io::Result<Self>;
      pub fn get_or_create_compute_pipeline(&self, device: &wgpu::Device, label: &str, shader_source: &str, entry_point: &str, layout: Option<&wgpu::PipelineLayout>) -> Arc<wgpu::ComputePipeline>;
      pub fn stats(&self) -> (usize, usize);
  }

Use std::collections::hash_map::DefaultHasher for hashing shader_source. wgpu 0.20 API: device.create_shader_module, device.create_compute_pipeline. Cache shader source to disk at <dir>/<hash>_<entry_point>.wgsl on miss. Thread-safe via Mutex. Include all `use` statements. No external crates beyond wgpu and std.

Return ONLY the complete .rs file content, no markdown fences, no commentary.'

ESC_PROMPT=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 600 -X POST http://127.0.0.1:5001/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC_PROMPT, \"max_length\": 2048, \"temperature\": 0.2, \"top_p\": 0.9, \"rep_pen\": 1.05}" \
  > "$OUT_DIR/qwen_pipeline_cache.json" 2>&1

python3 -c "import json,sys; d=json.load(open('$OUT_DIR/qwen_pipeline_cache.json')); print(d.get('results',[{}])[0].get('text',''))" > "$OUT_DIR/qwen_pipeline_cache.rs" 2>"$OUT_DIR/qwen_pipeline_cache.err"
echo "Qwen done: $(wc -l < "$OUT_DIR/qwen_pipeline_cache.rs" 2>/dev/null) lines"
