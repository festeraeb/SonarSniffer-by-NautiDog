#!/bin/bash
# Dispatch to Qwen3.6-35B-A3B-MoE on P100 #1 (port 5002)
# Task: Wire the existing pipeline_cache.rs into pipeline_init.rs +
# main.rs so binary shader caches are loaded/saved across runs.
#
# The module exists. We just need the call sites.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

PROMPT='Output a Rust patch as TWO complete file replacements separated by `===== FILE: <path> =====` markers. NO <think> blocks, NO commentary, NO markdown fences.

CONTEXT — these files already exist, here is the relevant API:

src/pipeline_cache.rs (already shipped) provides:
  pub struct PipelineCache { ... }
  impl PipelineCache {
      pub fn load(device_name: &str) -> Self;
      pub fn create_wgpu_cache(&self, device: &wgpu::Device) -> Option<wgpu::PipelineCache>;
      pub fn save(&self, wgpu_cache: &wgpu::PipelineCache);
      pub fn is_warm(&self) -> bool;
  }

src/pipeline_init.rs:
  pub fn init_layer_pipelines(device: &wgpu::Device) -> LayerPipelines;
  internal `fn create_pipeline(device, label, source, bgl, cache: Option<&wgpu::PipelineCache>) -> wgpu::ComputePipeline`
  but every call passes `None` for cache.

src/main.rs around the device setup:
  let (device, queue) = adapter.request_device(...)
  ...
  let pipelines = cesarops_inference::pipeline_init::init_layer_pipelines(&device);

YOUR TASK:

FILE 1: src/pipeline_init.rs
  - Change `pub fn init_layer_pipelines(device: &wgpu::Device)` to
    `pub fn init_layer_pipelines(device: &wgpu::Device, cache: Option<&wgpu::PipelineCache>) -> LayerPipelines`
  - Pass `cache` through to ALL `create_pipeline(...)` calls (replacing the existing `None`).
  - Where pipelines are built directly via `device.create_compute_pipeline(...)` (the matvec_pc and matvec_vec4_pc and matvec_bias and DequantQ6KPipeline-style calls), set the `cache: cache` field instead of `cache: None` in the ComputePipelineDescriptor.

FILE 2: src/main.rs
  Just before the line `let pipelines = cesarops_inference::pipeline_init::init_layer_pipelines(&device);`, ADD:
    let pcache_disk = cesarops_inference::pipeline_cache::PipelineCache::load(&adapter_info.name);
    let pcache_wgpu = pcache_disk.create_wgpu_cache(&device);
    if pcache_disk.is_warm() {
        info!("Pipeline cache warm (cold-start pipeline build will skip recompilation)");
    }
  Change the next line to pass the cache:
    let pipelines = cesarops_inference::pipeline_init::init_layer_pipelines(&device, pcache_wgpu.as_ref());
  After ALL pipelines are built (right after the existing `info!("All pipelines compiled.");`), ADD:
    if let Some(c) = &pcache_wgpu { pcache_disk.save(c); }

Output the COMPLETE updated content of both files. Do not abbreviate. Do not use diffs.

Format: `===== FILE: src/pipeline_init.rs =====` newline, file content, `===== FILE: src/main.rs =====` newline, file content. End.'

ESC=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 1500 -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC, \"max_length\": 8192, \"temperature\": 0.1, \"top_p\": 0.85, \"rep_pen\": 1.1, \"stop_sequence\": [\"<think>\"]}" \
  > "$OUT_DIR/r5_qwen_pipeline_cache_wire.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r5_qwen_pipeline_cache_wire.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r5_qwen_pipeline_cache_wire.txt" 2>"$OUT_DIR/r5_qwen_pipeline_cache_wire.err"
echo "[Qwen3.6 r5] pipeline_cache_wire: $(wc -l < "$OUT_DIR/r5_qwen_pipeline_cache_wire.txt") lines"
