# Fork Plan — mistral-rs + candle Integration

## Step 1: Clone into workspace
```bash
cd /codebase/wreckhunter2000-1
mkdir -p crates
git clone https://github.com/EricLBuehler/mistral.rs.git crates/mistral-rs-fork
git clone https://github.com/huggingface/candle.git crates/candle-fork
```

## Step 2: Strip mistral-rs (keep only)
- mistral-rs-core/src/pipeline/mod.rs (coordinator)
- mistral-rs-core/src/sequence.rs (token tracking)
- mistral-rs-core/src/models/qwen2_5.rs or qwen3.rs (model graph)
- mistral-rs-core/src/models/gdn.rs (Gated Delta Network)

DELETE: mistralrs-pyo3, mistralrs-server, vision, multi-modal

## Step 3: Patch candle wgpu backend
File: candle-core/src/wgpu_backend.rs (~line 30-50)
Add:
```rust
impl WgpuStorage {
    pub fn raw_buffer(&self) -> &Arc<wgpu::Buffer> {
        &self.buffer
    }
}
```

## Step 4: Quick viability test (CUDA first)
```bash
cd crates/mistral-rs-fork
cargo build --release --features cuda
./target/release/mistralrs run \
  --format gguf \
  --file /codebase/models/Qwen3.6-35B-A3B-MXFP4_MOE.gguf \
  --isq Q4K \
  --port 5001
```

## Step 5: Wire our modules
- cesarops-inference depends on candle-core (path = "../crates/candle-fork/candle-core")
- cesarops-inference depends on mistral-rs-core (path = "../crates/mistral-rs-fork/mistral-rs-core")
- Inject CakeKVPager into pipeline/mod.rs
- Inject WgpuInjection into the forward pass
- Wire geo_filter to tensor intercept

## Success Criteria
1. mistral-rs loads our 35B GGUF on P100s
2. Generates tokens (any speed)
3. Our Cake KV pager intercepts and pages to DDR4
4. Our geo_filter receives live tensor data
5. forge-v2 works without changes (same API on :5001)
