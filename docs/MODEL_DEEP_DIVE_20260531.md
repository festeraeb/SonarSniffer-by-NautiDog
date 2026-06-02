# Model deep dive (2026-05-31)

## Host constraints
- GPUs:
  - NVIDIA RTX 2060 SUPER (8 GB VRAM)
  - NVIDIA GTX 1070 (8 GB VRAM)
  - NVIDIA P106-100 (6 GB VRAM)
- CPU: dual Xeon E5-2440 (24 threads)
- RAM: 78 GiB
- Free disk:
  - /data free: ~65 GB
  - / free: ~750 GB

## Existing local models found
- /data/models/Qwen3.6-27B-IQ4_XS-mtp.gguf
- /data/models/Qwen3.6-27B-IQ3_M-mtp.gguf
- /data/models/Qwen2.5-7B-Instruct-Q4_K_M.gguf
- /data/models/Qwen2.5-3B-Instruct-Q4_K_M.gguf

## Candidate public models selected
All from public Hugging Face repos with direct download URLs.

| Role target | Model file | Approx size | Why this fits your tasks |
|---|---|---:|---|
| Coder primary | qwen2.5-coder-7b-instruct-q4_k_m.gguf | 4.36 GiB | Strong code-edit fidelity for shell/Rust/Python edits under 8 GB VRAM. |
| Thinker/planner | Qwen3-8B-Q4_K_M.gguf | 4.68 GiB | Better planning and instruction-following than smaller 3B/7B baselines. |
| Reviewer hard mode | DeepSeek-R1-Distill-Qwen-14B-IQ4_XS.gguf | 7.56 GiB | Strong reasoning/review quality; use with GPU+CPU offload if needed. |
| Reviewer alternative | gemma-2-9b-it-IQ4_XS.gguf | 4.83 GiB | Good critique/comparison behavior with lower latency than 14B. |
| Fast helper/tool model | Phi-4-mini-instruct.Q4_K_M.gguf | 2.32 GiB | Low-latency fallback for command synthesis and triage loops. |

Planned candidate bundle total: ~23.75 GiB.

## Best likely task mapping for your Forge work
- P1 shell invocation fixes and scripted ops:
  - Best first: Qwen2.5-Coder-7B-Instruct (coder)
  - Best second: Phi-4-mini-instruct (fast helper)
- P2 thinker latency/reliability tuning and route policy:
  - Best first: Qwen3-8B (thinker)
  - Best second: gemma-2-9b-it (reviewer alt)
- Strict reviewer grading and failure-mode catch:
  - Best first: DeepSeek-R1-Distill-Qwen-14B
  - Best second: gemma-2-9b-it

## Download method
- Script: scripts/download_model_candidates_20260531.sh
- Default output path: /data/models/candidates/20260531
- Resume support: yes (wget -c)
