# CESAROPS Lessons Learned — Auto-indexed by nautivecs

## [qwen3,think,fix,prompt] Qwen3 Think Block Pre-fill

Qwen3.6 models default to generating <think>...</think> blocks before responding. This causes empty responses when the think block is stripped. FIX: Pre-fill the think block in the prompt as `<think>\n</think>\n` right before the assistant turn. The model sees it already "thought" and jumps straight to content generation.

## [koboldcpp,stop_sequence,tool_calling] Synthetic Tool Calling with KoboldCPP

KoboldCPP 1.112.2 does NOT support /v1/chat/completions with tool_calls. Use the raw /api/v1/generate endpoint with stop_sequence: ["</tool_call>", "<|im_end|>"] to simulate tool calling. The model emits <tool_call>JSON</tool_call> and stops. The forge parses the XML, executes the tool, feeds the result back as <tool_result>...</tool_result>.

## [repetition,loop,fix] Repetition Loop Detection

When a model calls the same tool with the same arguments twice in a row, it's stuck. FIX: Detect duplicate tool calls, inject a system constraint ("You MUST NOT call this again"), spike temperature to 0.7 with wider top_p (0.95) to force exploration of alternative paths.

## [translator,model_format,normalization] Universal Translator Pattern

Different models use different formats for reasoning: Qwen uses <think>, DeepSeek uses <reasoning>, some use (thought). The translator (warp-grid/src/translator.rs) strips ALL known reasoning tags, extracts tool calls regardless of format, and normalizes into a standard NormalizedMessage struct. Add new model formats by adding one regex pattern.

## [numa,pinning,performance] NUMA Pinning Rules for T440

Socket 0 (Cores 0-7, HT 16-23) feeds P100 #0. Socket 1 (Cores 8-15, HT 24-31) feeds P100 #1. NEVER let Socket 0 threads feed P100 #1 — UPI penalty is 40% bandwidth loss. Use libc::sched_setaffinity(0, ...) with CPU_SET to pin threads. Read /sys/bus/pci/devices/*/numa_node to map GPUs to sockets.

## [avx512,throttle,xeon] AVX-512 Frequency Throttle Guard

Xeon Silver 4110: if 9+ cores execute AVX-512 FMA simultaneously, the ENTIRE chip drops to 1.4GHz. Limit AVX-512 rayon pool to 8 threads max. Use AVX2 (_mm256_fmadd_ps) for latency-sensitive work like drift correction — no frequency penalty.

## [nauticuvs,precision,f64] nauticuvs f64 Default Precision

nauticuvs now defaults to f64 (Scalar = f64) for all curvelet math. This eliminates phase accumulation errors in temporal stacking (4 pixels of drift in f32 → 0.0001 pixels in f64). The Xeons handle f64 at full speed via AVX-512. GPUs only see f32/f16 data (downcast at the staging boundary via GridBuffer::migrate()).

## [p100,fp16,half2] P100 FP16 2:1 Throughput

P100 (SM 6.0) has native 2:1 FP16 throughput — ~21 TFLOPS vs ~10.6 TFLOPS FP32. Use `enable f16;` in WGSL shaders with vec2<f16> storage buffers. The 1070/P1000/P106 (SM 6.1) have 1:64 FP16 ratio — NEVER use FP16 on these cards, always fall back to f32.

## [register_pressure,p100,shader] P100 Register Pressure Rule

Pascal GP100: 65536 registers / max 2048 threads = 32 registers per thread at full occupancy. If a shader uses >20 named variables, split into two passes. Count var/let declarations in WGSL source as a heuristic. Reject shaders with >32 vars for P100 targets.

## [nvml,driver,580] NVML Incompatible with Driver 580.x

nvml-wrapper crate does NOT work with NVIDIA driver 580.x (Pascal legacy branch). Use nvidia-smi --query-gpu=... --format=csv,noheader,nounits and parse the CSV output instead. This is reliable across all driver versions.

## [forge,tool_loop,architecture] Forge Tool-Calling Architecture

The forge uses a synthetic tool loop: generate with stop_sequence → parse <tool_call> → execute → feed <tool_result> back → generate again. Max 12 rounds. Detect repeated calls and force summary. Pre-fill <think></think> to skip Qwen's thinking mode. Temperature spike (0.7) + wider top_p (0.95) on repeat detection to force exploration.

## [thinker,8b,spark] Thinker-First Pattern

Always hit the 8B DeepSeek-R1 on the 1070 BEFORE the 35B executes. The R1 distillation gives chain-of-thought reasoning that approaches problems from angles the MoE wouldn't. The thinker's output is injected as [Thinker Analysis] in the 35B's prompt. Skip with /fast prefix when speed matters more than depth.

## [gridbuffer,memory,unified] Unified Memory Architecture

GridBuffer abstracts memory location (Host/GPU/RAID/Remote) and precision (FP16/FP32/FP64). Call migrate(target) to move data anywhere with automatic precision casting. The system auto-selects: f16 for Pascal GPUs, f64 for Xeon, compressed for network. RAID files are Tier 2 "slow VRAM" via memory-mapped access.

## [kline,security,guard] K-Line Security Guard

Block dangerous commands in run_command tool: rm -rf, dd if=, writes to /etc/. The forge's run_command checks for these patterns before execution. For peer banning: 3 strikes → K-line (local ban), 5 → G-line (global), 7 → Z-line (kernel firewall drop via nftables).

## [forge-v2,tool_call,json_fix] Qwen3.6 Malformed Tool Call JSON

Qwen3.6-35B-A3B produces malformed tool call JSON where it drops the "arguments" key name. Pattern: `{"name": "think_harder", {"arguments": {"query": "..."}}}` instead of `{"name": "think_harder", "arguments": {"query": "..."}}`. FIX: The translator's parse_tool_json now has a 3-tier fallback: (1) direct JSON parse, (2) regex-based fix for Qwen's comma-instead-of-colon pattern, (3) regex extraction of name + arguments block independently. This catches 95%+ of Qwen's malformed outputs.

## [forge-v2,architecture,self_healing] Self-Healing Knowledge Translator (SHKT) Architecture

cesarops-forge-v2 implements a 5-layer self-healing loop: (1) Thinker pre-flight (8B on cesarops2:5555), (2) Translator formats QwenChatML prompt with /no_think, (3) 35B generates on P100s via KoboldCPP:5001, (4) Translator normalizes + detects failures, (5) Diagnostic 8B analyzes failures and provides prompt overrides. Max 12 tool rounds, max 2 diagnosis attempts, then hard reset. Successful fixes auto-saved to nautivecs.

## [forge-v2,deployment,port] Forge-v2 Deployment on T440

cesarops-forge-v2 runs on port 9100 (replaces old cesarops-forge-web). Binary at /codebase/wreckhunter2000-1/cesarops-forge-v2/target/release/cesarops-forge-v2. Start with: `cd /codebase/wreckhunter2000-1/cesarops-forge-v2 && RUST_LOG=info nohup ./target/release/cesarops-forge-v2 > /tmp/forge-v2.log 2>&1 &`. Must stop old cesarops-forge-web.service first (sudo systemctl stop cesarops-forge-web).

## [forge-v2,tool_result,qwen_format] Tool Results as QwenChatML User Messages

Qwen3.6 responds better when tool results come as user messages in ChatML format, NOT custom XML tags. Format: `<|im_end|>\n<|im_start|>user\n[Tool Result - Round N/M]: {result}\nNow continue.<|im_end|>\n<|im_start|>assistant\n`. This gives the model a clear "your turn" signal after receiving tool output.

## [r1,tool_calling,failure,lesson] DeepSeek-R1-32B Refuses to Use Tools

DeepSeek-R1-32B-Q4_K_M on dual P100s was given search tools (think_harder, search_web, search_code) with explicit format examples. It DESCRIBED what it would search for but never emitted the actual <tool_call> JSON. It produced a 4-step research plan then stopped without executing any of it. The human had to run the searches manually and feed the results back. LESSON: R1 distillations may not reliably follow tool-calling schemas even with explicit examples. The 14B Coder (Fortytwo_Strand) should be the tool-calling agent, not R1. R1 is best used as a pure reasoning engine with pre-fetched context, not as an agentic tool user.

## [research,inference,tiered_kv,validated] Tiered KV Cache Validated by 2024-2026 Research

FlexGen (Stanford 2023): 1 tok/s on 175B model with single 16GB GPU + disk offloading. IBM 2025: Bottleneck is CPU-to-GPU path, NOT SSD read speed. NUMA DDR4 staging buffer solves this. NeurIPS 2025: DDR4 as prefetch buffer between RAID and GPU eliminates bandwidth bottleneck. InstInfer (2024): Offloads attention + KV to storage drives. Our 4-tier hierarchy (HBM2 32GB + GDDR5 8GB + DDR4 92GB + RAID 4TB) is validated by all of this research.

## [research,crates,wgpu_inference] Key Rust Crates for Native Inference

wgpu-llm-cli: LLM inference on ANY GPU via wgpu, no CUDA required (our exact approach). llama-gguf: Pure Rust GGUF loader with full format support. burn-wgpu: Burn's WGPU backend. candle-vllm: Efficient serving with OpenAI API. CubeCL/CubeK (Burn 0.20): Unified CPU/GPU kernels, 5x faster than standard channels.

## [research,speculative_decoding,p100] Speculative Decoding on Our Hardware

Use 1070 (8GB) with a 1.5B draft model to propose tokens. P100s verify in parallel. Expected 2-3x speedup. Self-speculative decoding uses MoE early layers as draft (no separate model needed). Disaggregated inference: P100s prefill (heavy math), 1070/P106 decode (lighter). Prevents memory wall.

## [hardware,register_pressure,optimization] P100 Register-Heavy Kernel Strategy

P100 has 256KB register file per SM. Load 16x16 tiles entirely into registers, bypass HBM2 bus latency for the inner matmul loop. This is the "sub-pixel alignment" trick — keep the hot data in registers, only touch HBM2 for loading the next tile.

## [aeromag,detection,classification] Geological Subtraction + Aspect Ratio Classification

Pipeline for wreck detection in Lake Erie: 1) Bandpass filter removes geological noise (basalt flows = low freq, glacial till = high freq noise). 2) What remains is the anthropogenic residual. 3) Lower detection threshold on clean residual to find weak galvanic dipoles. 4) Apply aspect ratio correction to classify: horizontal elongation = wreck, vertical = wellhead/pipes. 5) Cross-reference with thermal + blue spectrum + surface current ripples. The Rossa's lead keel + steel bolts create a persistent weak dipole that becomes visible ONLY after geological subtraction.

## [moe_review,inference,improvements] Qwen3.6 MoE Review of cesarops-inference Spec

Key additions from MoE review (May 2026): 1) FlashAttention in WGSL — O(n) tiling, critical for 32K+ context on P100. 2) INT8 KV quantization for cold tiers — doubles context window. 3) Zero-allocation inference loop — pre-allocate all buffers, arena allocators, mimalloc for NUMA. 4) Memory tokens — summarize old context instead of evicting (Deep Time memory). 5) Domain-specific grammar constraints — valid lat/lon, depth ranges for Great Lakes. 6) Early-exit for simple queries. 7) Per-token telemetry. REJECTED: Drop QUIC (it's cross-node not intra-GPU), CUDA backend (locks to NVIDIA), continuous batching (single-user).

## [wgpu-llm,dashboard,cesarops2] wgpu-llm Dashboard Launch on cesarops2

Location: ~/benchmark/wgpu-llm on cesarops2 (100.102.158.111)
Port: 8085 (http://100.102.158.111:8085/)
Model dir: ~/fake_model (skeleton Llama model with TinyLlama tokenizer)
Launch command: HOST=0.0.0.0 PORT=8085 cargo run --bin wgpu-llm -- --model-dir ~/fake_model --prompt "" --max-tokens 0 --temperature 0
Prerequisites: 1) Run python3 ~/create_fake_model.py to create skeleton model. 2) Download TinyLlama tokenizer: wget https://huggingface.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0/resolve/main/tokenizer.json -O ~/fake_model/tokenizer.json. 3) Mount Samba: sudo mount -t cifs //100.72.182.77/cesarops-external /mnt/data-external -o username=cesarops,password=cesarops,vers=3.0
