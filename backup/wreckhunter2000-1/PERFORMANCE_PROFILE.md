# KoboldCPP Performance Profile

## Hardware Configuration
- **Server**: Xeon (100.102.158.111)  
- **GPU**: NVIDIA GeForce GTX 1070 (8GB VRAM)
- **CPU**: Intel Xeon (32GB system RAM)
- **Model**: Qwen2.5-Coder-32B-Instruct-Q3_K_M (32 billion parameters)
- **Quantization**: Q3_K_M (3-bit, ~12GB uncompressed)
- **GPU Layers**: 11 out of 65 (due to 8GB VRAM constraint)

## Performance Metrics

### First-Run (Cold Start)
- **CUDA Kernel Compilation**: ~30-60 seconds (one-time)
- **Model Warm-up**: ~1-2 seconds  
- **Prompt Encoding**: ~0.5-2 seconds per batch
- **Token Generation**: ~0.8-1.5 tokens/second (depends on prompt length)

### Subsequent Runs
- **Warm-up**: Cached (~100ms)
- **Token Generation**: ~1-1.5 tokens/second

## Inference Speed Analysis

### Current Configuration (11 GPU layers, 54 CPU layers)
```
Throughput: 1.37 tokens/second
Time per token: ~730ms

Example Generation:
- Prompt: "def fibonacci(n):" (4 tokens)
- Generate: 100 tokens
- Processing time: 2.09s
- Generation time: 72.88s
- Total: 75.08s
```

### Why is it slow?
1. **Model Size**: 32B parameters = very large model
2. **Limited GPU Memory**: Only 11/65 layers on GPU, 54 layers on CPU
3. **PCIe Bottleneck**: GPU↔CPU transfers for layer computation
4. **Q3_K Quantization**: Requires more dequantization than higher-bit quantized models

### Optimization Options

#### Option 1: Use Smaller Model (Recommended)
- Qwen2.5-Coder-7B (7B parameters)
- Expected: 8-15 tokens/second
- Memory: ~3.5GB GPU
- Trade-off: Lower quality responses

#### Option 2: Use Smaller Model on p1000 too
- DeepSeek-Coder-1.3B  
- Expected: 20-30 tokens/second
- Memory: ~1.5GB
- Trade-off: Much lower quality

#### Option 3: Offload More to GPU
- Requires higher-bit quantization (Q4, Q5)  
- Model size: ~15-18GB
- Would exceed 8GB GPU
- Not viable for Xeon GTX 1070

#### Option 4: Pair with p1000 for Distributed Inference
- Split layers across Xeon (GTX 1070) + p1000 (P106)
- Theoretical: ~8-11 total tokens/sec
- Requires: Model download + network setup

## Practical Recommendations

### For VSCode AI Assistant
**Current Setup (32B model): ✅ Works but slow**
- Best for: Code explanation, detailed analysis, complex tasks
- Use case: When code quality matters more than speed
- Expected: 5-10 second latency for simple completions

**Recommended: Use 7B Model**
```bash
# Stop current
.venv\Scripts\python.exe deploy_kobold_multi.py --action stop --machines xeon

# Download Qwen2.5-Coder-7B on Xeon:
ssh cesarops1@100.102.158.111 "cd ~/ai_coding/models && \
  huggingface-cli download Qwen/Qwen2.5-Coder-7B-Instruct-GGUF \
  qwen2.5-coder-7b-instruct-q4_k_m.gguf --local-dir . --local-dir-use-symlinks False"

# Redeploy with 7B model
.venv\Scripts\python.exe deploy_kobold_multi.py --action deploy --machines xeon \
  --model /home/cesarops1/ai_coding/models/qwen2.5-coder-7b-instruct-q4_k_m.gguf
```

### Expected Performance Improvement (7B model)
- **Token Generation**: 8-15 tokens/second (10x faster!)
- **Simple completion**: ~1-2 seconds
- **Code explanation**: ~2-3 seconds  
- **Detailed analysis**: ~5-10 seconds

## Monitoring & Profiling

### Check Generation Speed
```bash
ssh cesarops1@100.102.158.111 "tail -n 1 ~/koboldcpp-5001.log"
# Look for "Generated: X/Y tokens" and "T/s"
```

### Monitor Real-time
```bash
ssh cesarops1@100.102.158.111 "tail -f ~/koboldcpp-5001.log | grep -E 'Generated|CtxLimit'"
```

### Full Log Analysis
```bash
ssh cesarops1@100.102.158.111 "cat ~/koboldcpp-5001.log | \
  grep 'Generated' | \
  awk '{print $NF}' | \
  sort | uniq -c | sort -rn"
```

## API Usage Guidance

### Reasonable Timeouts
```python
# Based on observed performance:
# - Cold start: 120 seconds
# - Warm completion: 30 seconds per request
# - Max tokens: 50-100 for interactive use

requests.post(
    url,
    json=payload,
    timeout=120  # First request
)

requests.post(
    url,
    json=payload,  
    timeout=60  # Subsequent requests
)
```

### Optimal Payload for Speed
```python
{
    "prompt": "your code",
    "max_tokens": 50,        # Shorter is faster
    "temperature": 0.7,       # Lower = more deterministic
    "top_p": 0.9,
    "top_k": 40
}
```

## Integration with cesar_agent_llm.py

### Current Limitations
- API latency: 5-75 seconds per request
- Not suitable for: Real-time chat, live code suggestions
- Suitable for: Offline analysis, batch processing, detailed reviews

### Usage Pattern
```python
# Good: Use for complex tasks
async def analyze_large_code_block(code):
    response = await kobold_complete(
        f"Analyze this code for bugs:\n{code}",
        max_tokens=200,
        timeout=60
    )

# Bad: Don't use for simple completions
async def auto_complete_next_line():
    # This will be too slow for interactive use
    response = await kobold_complete(...)
```

## Future Improvements

### Short-term
- [ ] Deploy 7B model for better performance
- [ ] Tune quantization for speed vs quality tradeoff
- [ ] Implement request queuing for batch processing

### Medium-term  
- [ ] Setup p1000 with smaller model for parallel inference
- [ ] Implement speculative decoding for faster generation
- [ ] Cache common prompts/responses

### Long-term
- [ ] Train custom quantized model for code-specific tasks
- [ ] Implement dynamic model loading (swap 32B ↔ 7B based on need)
- [ ] Setup distributed inference pipeline across Xeon + p1000

---

## Conclusion

✅ **KoboldCPP is fully operational and generating correct responses**

🚀 **Performance is acceptable for non-interactive use**

⚡ **Upgrade to 7B model recommended for production use** (10x speed improvement)

🎯 **Perfect for local AI-powered code analysis without Copilot dependency**
