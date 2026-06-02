# ZAYA1 on cesarops2 (2060 + 1070)

Zyphra **ZAYA1-8B** is MoE (~760M active / 8.4B total). Stock vLLM/llama.cpp do not support it — use [Zyphra/vllm `zaya1-pr`](https://github.com/Zyphra/vllm/tree/zaya1-pr).

| GPU | Role | Notes |
|-----|------|--------|
| RTX 2060 SUPER (1) | Primary Turing | Native sm_75 kernels |
| GTX 1070 (2) | Second EP rank | sm_61; pair with 2060 via `-dp 2 --enable-expert-parallel` |
| P106 (0) | **Skipped** | 6 GB — not enough for MoE shard + KV in practice |

**ZAYA1-VL-8B** (vision) uses `transformers@zaya1-vl`, not this vLLM fork. Download both; run VL from `install_zaya_vl_transformers.sh` when needed.

## One-time setup

```bash
bash scripts/zaya/download_zaya_models.sh      # ~20GB+ each
bash scripts/zaya/install_zaya_vllm_c2.sh      # 30–90 min compile
# optional VL runtime:
bash scripts/zaya/install_zaya_vl_transformers.sh
```

## Serve text model (2-GPU MoE)

Free llama on 2060/1070 first if VRAM is tight:

```bash
FREE_LLAMA=1 bash scripts/zaya/serve_zaya_2060_1070.sh
```

Test: `curl http://127.0.0.1:8010/v1/models`

## CPU fallback

If 2-GPU EP OOMs, single-GPU options: stop other GPU jobs, or `transformers` + `device_map=auto` with CPU offload (slow). vLLM DP+EP on 2060+1070 is the intended path.

## T440 dual P100

```bash
bash scripts/zaya/download_zaya_models.sh   # bf16 weights on SAS
bash scripts/zaya/install_zaya_vllm_p100.sh # sm_60 build, 30–90 min
SERVE_MODE=vllm bash scripts/zaya/serve_zaya_p100.sh   # :5001 DP+EP
```

**MXFP4 note:** [OsaurusAI/ZAYA1-8B-MXFP4](https://huggingface.co/OsaurusAI/ZAYA1-8B-MXFP4) is an Apple Silicon (MLX) bundle, not CUDA/vLLM. On P100 use vLLM bf16 (both cards) or `barozp/ZAYA1-8B-BNB` NF4 when arch matches. Transformers `fp16_auto` + CPU offload is a debug fallback only (`SERVE_MODE=fp16`).
