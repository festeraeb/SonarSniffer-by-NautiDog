"""Feed spec to R1-32B one question at a time. Each answer saved immediately."""
import json
import urllib.request
import sys
import time

R1_URL = "http://127.0.0.1:5557/api/v1/generate"
OUTPUT_DIR = "/codebase/wreckhunter2000-1/docs/r1_answers"

CONTEXT = """You are assessing a native Rust LLM inference engine spec for Pascal-era GPUs (P100).
Hardware: 2x P100 16GB HBM2, 2x Xeon 4110 AVX-512, 92GB DDR4, 1070 8GB, RAID.
The engine replaces KoboldCPP with Burn framework + wgpu, zero-copy GridBuffer memory,
distributed KV cache (HBM2->GDDR5->DDR4->RAID), custom matmul_half2.wgsl for FP16 2:1.
Mission: Maritime SAR shipwreck detection. Same GPU runs LLM + sonar/satellite processing.
You have web search at http://127.0.0.1:5010/search if needed."""

questions = [
    ("q1_risks", "What are the biggest RISKS in this plan? What could fail or be harder than expected? Focus on: Burn+wgpu maturity for Pascal, GGUF mmap loading complexity, zero-copy bridge feasibility, and whether wgpu 29.x actually supports f16 storage buffers on Vulkan/Pascal."),
    ("q2_strengths", "What STRENGTHS does our hardware have that we might be underutilizing? Think about: 92GB DDR4 as massive KV overflow, dual-socket NUMA for parallel workloads, AVX-512 for CPU-side preprocessing, HBM2 732GB/s bandwidth, and the 4TB total RAID as a memory tier."),
    ("q3_cutting_edge", "What cutting-edge AI inference techniques from 2024-2026 could work on Pascal GPUs in a pure Rust environment? Consider: speculative decoding with a small draft model, ring attention for long context, paged attention (vLLM-style), Multi-Head Latent Attention (MLA from DeepSeek), and INT8 KV quantization."),
    ("q4_crates", "Are there any Rust crates or open-source projects doing similar work - native LLM inference on older GPUs, wgpu compute shaders for ML, or distributed inference in Rust? What can we learn from or build on?"),
    ("q5_performance", "What realistic token/s can we expect from a 35B MoE (only 3B active params per token) on dual P100s with custom FP16 matmul shaders? What will be the bottleneck - memory bandwidth, compute, or the MoE routing overhead?"),
]

import os
os.makedirs(OUTPUT_DIR, exist_ok=True)

# Get which question to ask from command line (default: 1)
q_num = int(sys.argv[1]) if len(sys.argv) > 1 else 1
if q_num < 1 or q_num > len(questions):
    print(f"Usage: python3 {sys.argv[0]} [1-{len(questions)}]")
    sys.exit(1)

filename, question = questions[q_num - 1]

prompt = f"""<|im_start|>system\n{CONTEXT}<|im_end|>\n<|im_start|>user\n{question}<|im_end|>\n<|im_start|>assistant\n"""

payload = json.dumps({
    "prompt": prompt,
    "max_length": 512,
    "temperature": 0.7,
    "top_p": 0.95,
    "rep_pen": 1.1,
    "stop_sequence": ["<|im_end|>"],
}).encode()

print(f"Q{q_num}: {question[:80]}...")
print(f"Sending to R1-32B (expect 5-10 min at CPU speed for 512 tokens)...")
start = time.time()

req = urllib.request.Request(R1_URL, data=payload, headers={"Content-Type": "application/json"}, method="POST")
try:
    resp = json.loads(urllib.request.urlopen(req, timeout=900).read())
    text = resp["results"][0]["text"]
    elapsed = time.time() - start
    
    # Save immediately
    out_path = f"{OUTPUT_DIR}/{filename}.md"
    with open(out_path, "w") as f:
        f.write(f"# R1 Answer: Q{q_num}\n\n")
        f.write(f"**Question:** {question}\n\n")
        f.write(f"**Generated in:** {elapsed:.0f}s (~{len(text.split())/(elapsed+0.1):.1f} tok/s)\n\n")
        f.write("---\n\n")
        f.write(text)
    
    print(f"\nDONE in {elapsed:.0f}s ({len(text)} chars)")
    print(f"Saved to: {out_path}")
    print(f"\n{'='*60}")
    print(text)
    print(f"{'='*60}")
    
except Exception as e:
    print(f"ERROR: {e}")
    sys.exit(1)
