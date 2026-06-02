# 2060 Small Model Benchmark

- Host: `10.0.0.201:5200`
- Scope: <=8GB models + turbo-MoE offload stretch

| Model | Score | Code s | Tool s | Boot |
|---|---:|---:|---:|---|
| `Phi-3-mini-4k-instruct-Q4_K_M.gguf` | 11 | 2.93 | 1.54 | ok |
| `Qwen3.6-35B-A3B-MXFP4_MOE.gguf` | 11 | 71.95 | 16.12 | ok |
| `TinyLlama-1.1B-Chat-v1.0-Q4_K_M.gguf` | 9 | 1.36 | 0.66 | ok |
| `gemma-4-E4B-it-Q4_K_M.gguf` | 9 | 4.12 | 1.04 | ok |
| `qwen2.5-coder-1.5b-instruct-q6_k.gguf` | 8 | 2.04 | 0.38 | ok |
| `Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf` | 6 | 5.39 | 4.98 | ok |
| `DeepSeek-R1-Distill-Qwen-7B-Uncensored.Q4_K_M.gguf` | 5 | 4.45 | 4.19 | ok |
