"""Feed the full Burn/Cake inference spec to DeepSeek-R1-32B for assessment."""
import json
import urllib.request

R1_URL = "http://127.0.0.1:5557/api/v1/generate"

# The full spec + directive
prompt = """<|im_start|>system
You are DeepSeek-R1, a deep reasoning AI running on Xeon Silver 4110 CPUs with 92GB DDR4.
You have access to a Web Search Oracle (WSO) at http://127.0.0.1:5010/search for researching current AI techniques.
You have access to nautivecs (vector knowledge base with 12,600+ code chunks) at http://127.0.0.1:5003/query.

Your task: Assess the following inference engine spec, research the newest cutting-edge AI inference work,
and suggest improvements. Note strengths and weaknesses for our specific hardware (or similar equipment).

Take your time. Think deeply. This is a one-shot research task.
<|im_end|>
<|im_start|>user
## HARDWARE WE HAVE:
- 2x Tesla P100 16GB HBM2 (SM 6.0, native FP16 2:1 = 19 TFLOPS)
- 2x Xeon Silver 4110 (8 cores each, AVX-512, 92GB DDR4)
- 1x GTX 1070 8GB GDDR5 (SM 6.1, NO useful FP16)
- 1x P1000/P106 4-6GB (small GPU)
- RAID storage: 465GB + 1.8TB + 916GB
- NUMA dual-socket topology
- All connected via Tailscale (QUIC capable)

## THE SPEC: cesarops-inference (Native Rust LLM Inference Engine)

### Vision
Replace KoboldCPP with a pure Rust inference engine built on Burn + warp-grid's unified memory.
Zero PCIe hops between inference and compute. The same HBM2 that runs dipole detection also runs
token generation. Hardware-interrogating — adapts to whatever silicon it finds.

### Requirements:
1. GGUF Model Loading into GridBuffer (mmap from RAID, MXFP4/Q4_K_M/Q8_0 support, MoE expert sharding across GPUs)
2. GridBuffer <-> Burn Tensor Bridge (zero-copy, precision-aware, bidirectional)
3. Transformer Forward Pass via Burn (universal template, custom matmul_half2.wgsl for P100, fallback matmul_f32.wgsl for 1070)
4. KV Cache as GridBuffer (HBM2 Tier 0, DDR4 Tier 1 overflow, NUMA-aware, ring buffer)
5. Rust-native Tokenizer (Qwen models, special tokens for tool calling)
6. Sampling (temperature, top_p, rep_pen, stop sequences, logit bias to kill <think> tokens)
7. HTTP API (KoboldCPP-compatible drop-in replacement)
8. Hardware Interrogation at Startup (detect GPUs, classify capabilities, build ModelSpec)
9. Concurrent Inference + Compute (shared wgpu Device, mode switching between LLM and SAR scan)

### Key Design Decisions:
- Distributed KV "Cake" Protocol: Tier 0 (HBM2) -> Tier 1 (GDDR5 via QUIC) -> Tier 2 (DDR4) -> Tier 3 (RAID mmap)
- Kernel Fusion: Burn-wgpu fuses Attention + RoPE into single GPU pass
- Zero-Init Weight Mapping: pmetal-gguf mmap (model starts in milliseconds)
- Grammar-Constrained Sampler: Force valid JSON tool calls at the logit level
- Multi-Head Latent Attention (MLA): Compress KV heads before shipping over QUIC (~4x reduction)
- INT8 KV Quantization for evicted blocks (~4x storage reduction)

### The Mission Context:
This engine powers an autonomous maritime SAR (Search and Rescue) system detecting shipwrecks
from satellite imagery. The same P100 HBM2 that runs the LLM also runs dipole detection shaders
on sonar/satellite data. Zero-copy between "thinking" and "scanning" is critical.

## YOUR TASK:
1. Assess this plan — what's solid, what's risky, what's missing?
2. Research the newest cutting-edge AI inference techniques (2024-2026) that could apply to Pascal-era GPUs in a Rust-only environment
3. Suggest improvements we might not be thinking of
4. Note strengths and weaknesses of our specific hardware for this workload
5. Are there any similar open-source projects or crates we should look at?
6. What's the realistic token/s we can expect from a 35B MoE on dual P100s with this architecture?

Be thorough. Be creative. Think of things we haven't considered.
<|im_end|>
<|im_start|>assistant
"""

payload = json.dumps({
    "prompt": prompt,
    "max_length": 2048,
    "temperature": 0.7,
    "top_p": 0.95,
    "rep_pen": 1.1,
    "stop_sequence": ["<|im_end|>"],
}).encode()

print("Sending spec to R1-32B... (this will take several minutes at CPU speed)")
print(f"Prompt length: {len(prompt)} chars")

req = urllib.request.Request(R1_URL, data=payload, headers={"Content-Type": "application/json"}, method="POST")

try:
    resp = json.loads(urllib.request.urlopen(req, timeout=3600).read())  # 60 min timeout
    text = resp["results"][0]["text"]
    
    # Save output
    output_path = "/codebase/wreckhunter2000-1/docs/r1_spec_assessment.md"
    with open(output_path, "w") as f:
        f.write("# DeepSeek-R1-32B Assessment of cesarops-inference Spec\n\n")
        f.write(text)
    
    print(f"\n=== R1 ASSESSMENT COMPLETE ===")
    print(f"Length: {len(text)} chars")
    print(f"Saved to: {output_path}")
    print(f"\nFirst 1000 chars:")
    print(text[:1000])
    print("...")
    
except Exception as e:
    print(f"ERROR: {e}")
    import traceback
    traceback.print_exc()
