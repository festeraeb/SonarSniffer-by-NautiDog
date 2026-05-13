# CesarOps WGPU Engine: End-to-End Integration Checklist

## Status: 27 modules compiled with candle-core + wgpu. Ready for token generation.

## Phase 1: Storage Mapping & Weight Verification
- [ ] Verify GGUF at /codebase/models/Qwen3.6-35B-A3B-MXFP4_MOE.gguf
- [ ] Confirm InferenceArena locks 1GB on NUMA Node 0
- [ ] Validate loader.rs reads hidden_dim, vocab_size, n_layers correctly

## Phase 2: Tokenizer String-to-Buffer Alignment
- [ ] Load tokenizer.json (TinyLlama tokenizer on cesarops2, need Qwen tokenizer)
- [ ] Test prompt encoding → Vec<u32>
- [ ] Confirm [1, seq_len] matrix layout

## Phase 3: WebGPU Hardware Dispatch
- [ ] Device::new_wgpu(0) under wgpu-backend feature
- [ ] Dynamic binding context writes M,K,N to Binding 3 per step
- [ ] matmul_half2.wgsl runs without host stalling
- [ ] Async sync fence retrieves logits

## Phase 4: Output Emission & Detection Interception
- [ ] sampling.rs filters logits and selects token
- [ ] Real-time token streaming to console
- [ ] geo_filter.rs hook borrows memory handle for detection

## Next Steps When Returning:
1. Write wgpu_generator.rs using candle tensors
2. Wire main.rs to call it
3. Run: RUST_LOG=info cargo run --release --features "wgpu-backend"
4. Verify first token emits

## What's Running on T440:
- KoboldCPP (35B MoE): port 5001
- Forge-v2 (SHKT): port 9100
- nautivecs: port 5003
- cesarops-inference: compiles clean, 27 modules + candle + wgpu
