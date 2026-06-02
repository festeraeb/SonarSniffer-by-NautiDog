#!/usr/bin/env python3
"""ZAYA1-8B OpenAI-ish HTTP API via Hugging Face Transformers (no vLLM).

Modes (set ``ZAYA_LOAD_MODE``):

- ``fp16_auto`` — full weights in FP16 (Pascal: never bf16), ``device_map="auto"``
  across visible GPUs; optional ``max_memory`` JSON.
- ``bnb_nf4`` — on-the-fly NF4 + double-quant from official ZAYA1-8B weights,
  ``bnb_4bit_compute_dtype=float16``, ``device_map="auto"`` (recommended on 2×16GB).
- ``nf4`` — legacy: pre-quantized barozp checkpoint on GPU0 only.

Speed is not a goal — reliability is.

Environment:

- ``ZAYA_MODEL_DIR`` — model path (default: NF4 path for ``nf4``, must point at
  ``Zyphra/ZAYA1-8B`` for ``fp16_auto``).
- ``ZAYA_LOAD_MODE`` — ``fp16_auto`` | ``bnb_nf4`` | ``nf4`` (default: ``fp16_auto``).
- ``ZAYA_DEFAULT_TEMP`` / ``ZAYA_TOP_P`` / ``ZAYA_TOP_K`` — Zyphra sampling (math: 1.0/0.95/-1).
- ``ZAYA_MAX_MEMORY`` — JSON object for ``max_memory``, e.g.
  ``{"0":"7GiB","1":"7GiB","cpu":"30GiB"}`` (CUDA indices are *after*
  ``CUDA_VISIBLE_DEVICES`` remapping).
- ``ZAYA_USE_CACHE`` — ``0``/``1`` (default ``0`` = ``use_cache=False``, safer
  for Zaya CCA until upstream is fully aligned).
- ``ZAYA_API_HOST`` / ``ZAYA_API_PORT`` / ``ZAYA_MAX_NEW_TOKENS`` — server defaults.
"""
from __future__ import annotations

import json
import os
import time
import uuid
from contextlib import asynccontextmanager
from typing import Any, Optional

import torch
import uvicorn
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field
from transformers import (
    AutoConfig,
    AutoModelForCausalLM,
    AutoTokenizer,
    BitsAndBytesConfig,
    GenerationConfig,
)

_DEFAULT_FP16 = os.path.expanduser("~/models/Zyphra/ZAYA1-8B")
_DEFAULT_NF4 = os.path.expanduser("~/models/Zyphra/barozp-ZAYA1-8B-NF4/NF4")
LOAD_MODE = os.environ.get("ZAYA_LOAD_MODE", "fp16_auto").strip().lower()
MODEL_DIR = os.environ.get(
    "ZAYA_MODEL_DIR",
    _DEFAULT_NF4 if LOAD_MODE == "nf4" else _DEFAULT_FP16,
)
HOST = os.environ.get("ZAYA_API_HOST", "0.0.0.0")
PORT = int(os.environ.get("ZAYA_API_PORT", "8010"))
DEFAULT_MAX_NEW = int(os.environ.get("ZAYA_MAX_NEW_TOKENS", "256"))
MAX_PROMPT_TOKENS = int(os.environ.get("ZAYA_MAX_PROMPT_TOKENS", "2048"))
USE_CACHE = os.environ.get("ZAYA_USE_CACHE", "0").strip() in ("1", "true", "yes")
DEFAULT_TEMP = float(os.environ.get("ZAYA_DEFAULT_TEMP", "1.0"))
DEFAULT_TOP_P = float(os.environ.get("ZAYA_TOP_P", "0.95"))
_TOP_K_RAW = os.environ.get("ZAYA_TOP_K", "-1").strip()
DEFAULT_TOP_K: int | None = None if _TOP_K_RAW in ("", "-1", "none") else int(_TOP_K_RAW)

_model = None
_tokenizer = None


def _first_param_device(model: torch.nn.Module) -> torch.device:
    p = next(model.parameters(), None)
    if p is None:
        return torch.device("cuda:0" if torch.cuda.is_available() else "cpu")
    return p.device


def _input_device(model: torch.nn.Module) -> torch.device:
    """Device for `input_ids` — must match the embedding / first pipeline stage."""
    hf_map = getattr(model, "hf_device_map", None)
    if isinstance(hf_map, dict):
        for key in (
            "model.embed_tokens",
            "embed_tokens",
            "model.decoder.embed_tokens",
            "transformer.wte",
        ):
            if key in hf_map:
                loc = hf_map[key]
                if isinstance(loc, int):
                    return torch.device(f"cuda:{loc}")
                if isinstance(loc, str):
                    if loc.isdigit():
                        return torch.device(f"cuda:{loc}")
                    return torch.device(loc)
        for loc in hf_map.values():
            if isinstance(loc, int):
                return torch.device(f"cuda:{loc}")
            if isinstance(loc, str) and loc.startswith("cuda"):
                return torch.device(loc)
    return _first_param_device(model)


def _load_model():
    global _model, _tokenizer
    if _model is not None:
        return _model, _tokenizer

    if not os.path.isfile(os.path.join(MODEL_DIR, "config.json")):
        raise RuntimeError(f"Missing model at {MODEL_DIR}")

    print(f"[zaya-api] mode={LOAD_MODE} model_dir={MODEL_DIR}", flush=True)
    _tokenizer = AutoTokenizer.from_pretrained(MODEL_DIR, trust_remote_code=True)

    if LOAD_MODE == "fp16_auto":
        max_mem_raw = os.environ.get("ZAYA_MAX_MEMORY", "").strip()
        max_memory: dict[str, Any] | None = None
        if max_mem_raw:
            payload = max_mem_raw
            while True:
                try:
                    max_memory = json.loads(payload)
                    break
                except json.JSONDecodeError:
                    # Some launch contexts can accidentally append trailing "}".
                    if payload.endswith("}"):
                        payload = payload[:-1]
                        continue
                    raise
            # accelerate expects GPU keys as ints, plus optional "cpu"/"disk".
            max_memory = {
                (int(k) if isinstance(k, str) and k.isdigit() else k): v
                for k, v in max_memory.items()
            }
            # accelerate expects string keys like "0", "1", "cpu"
            print(f"[zaya-api] max_memory={max_memory}", flush=True)

        print("[zaya-api] loading FP16 with device_map=auto (may take minutes)…", flush=True)
        load_kw: dict[str, Any] = {
            "trust_remote_code": True,
            "torch_dtype": torch.float16,
            "device_map": "auto",
            "low_cpu_mem_usage": True,
            "attn_implementation": "eager",
        }
        if max_memory is not None:
            load_kw["max_memory"] = max_memory
        _model = AutoModelForCausalLM.from_pretrained(MODEL_DIR, **load_kw)
    elif LOAD_MODE == "bnb_nf4":
        # Pascal: NF4+DQ on GPU0 (~6GB); use bnb_nf4_dual + CUDA 0,1 to shard (experimental)
        bnb_map = os.environ.get("ZAYA_BNB_DEVICE_MAP", "auto").strip()
        print(
            f"[zaya-api] loading NF4+DQ on-the-fly (fp16 compute, device_map={bnb_map})…",
            flush=True,
        )
        bnb = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=torch.float16,
            bnb_4bit_use_double_quant=True,
        )
        _model = AutoModelForCausalLM.from_pretrained(
            MODEL_DIR,
            trust_remote_code=True,
            quantization_config=bnb,
            device_map=bnb_map,
            low_cpu_mem_usage=True,
            attn_implementation="eager",
        )
    elif LOAD_MODE == "nf4":
        config = AutoConfig.from_pretrained(MODEL_DIR, trust_remote_code=True)
        qconf = getattr(config, "quantization_config", None)
        pre_quantized = False
        if qconf is not None:
            if isinstance(qconf, dict):
                pre_quantized = bool(qconf.get("load_in_4bit"))
            else:
                pre_quantized = bool(getattr(qconf, "load_in_4bit", False))

        load_kwargs: dict[str, Any] = {
            "trust_remote_code": True,
            "device_map": {"": 0},
            "attn_implementation": "eager",
        }
        if pre_quantized:
            print("[zaya-api] loading pre-quantized NF4 checkpoint…", flush=True)
        else:
            load_kwargs["quantization_config"] = BitsAndBytesConfig(
                load_in_4bit=True,
                bnb_4bit_quant_type="nf4",
                bnb_4bit_compute_dtype=torch.float16,
                bnb_4bit_use_double_quant=True,
            )
            print("[zaya-api] loading NF4 (on-the-fly bitsandbytes)…", flush=True)
        _model = AutoModelForCausalLM.from_pretrained(MODEL_DIR, **load_kwargs)
    else:
        raise RuntimeError(
            f"Unknown ZAYA_LOAD_MODE={LOAD_MODE!r} (use fp16_auto, bnb_nf4, or nf4)"
        )

    _model.eval()
    print(f"[zaya-api] model ready use_cache_default={USE_CACHE}", flush=True)
    return _model, _tokenizer


@asynccontextmanager
async def lifespan(_app: FastAPI):
    _load_model()
    yield


app = FastAPI(title="ZAYA1-8B API", lifespan=lifespan)


class ChatMessage(BaseModel):
    role: str
    content: str


class ChatCompletionRequest(BaseModel):
    model: str = "ZAYA1-8B"
    messages: list[ChatMessage]
    max_tokens: Optional[int] = Field(default=None, ge=1, le=4096)
    temperature: float = Field(default=DEFAULT_TEMP, ge=0.0, le=2.0)
    stream: bool = False


class CompletionRequest(BaseModel):
    model: str = "ZAYA1-8B"
    prompt: str
    max_tokens: Optional[int] = Field(default=None, ge=1, le=4096)
    temperature: float = Field(default=DEFAULT_TEMP, ge=0.0, le=2.0)


def _truncate_prompt(prompt: str) -> str:
    model, tokenizer = _load_model()
    ids = tokenizer.encode(prompt, add_special_tokens=False)
    if len(ids) <= MAX_PROMPT_TOKENS:
        return prompt
    trimmed = tokenizer.decode(ids[:MAX_PROMPT_TOKENS], skip_special_tokens=True)
    print(
        f"[zaya-api] truncated prompt {len(ids)} -> {MAX_PROMPT_TOKENS} tokens",
        flush=True,
    )
    return trimmed


def _generate_text(prompt: str, max_new_tokens: int, temperature: float) -> str:
    model, tokenizer = _load_model()
    dev = _input_device(model)
    prompt = _truncate_prompt(prompt)
    inputs = tokenizer(prompt, return_tensors="pt")
    input_ids = inputs["input_ids"].to(dev)
    attention_mask = inputs.get("attention_mask")
    if attention_mask is None:
        attention_mask = torch.ones_like(input_ids, device=dev)
    else:
        attention_mask = attention_mask.to(dev)
    t0 = time.perf_counter()
    with torch.inference_mode():
        if temperature and temperature > 0:
            gen_cfg = GenerationConfig(
                do_sample=True,
                temperature=float(temperature),
                top_p=DEFAULT_TOP_P,
                top_k=None if DEFAULT_TOP_K is None or DEFAULT_TOP_K <= 0 else DEFAULT_TOP_K,
                max_new_tokens=max_new_tokens,
            )
        else:
            gen_cfg = GenerationConfig(
                do_sample=False,
                max_new_tokens=max_new_tokens,
            )
        use_cache = bool(USE_CACHE) or LOAD_MODE in ("bnb_nf4", "fp16_auto")
        gen_kwargs: dict[str, Any] = {
            "generation_config": gen_cfg,
            "use_cache": use_cache,
        }

        out_ids = model.generate(
            input_ids=input_ids,
            attention_mask=attention_mask,
            pad_token_id=tokenizer.pad_token_id,
            **gen_kwargs,
        )
    dt = time.perf_counter() - t0

    in_len = int(inputs["input_ids"].shape[1])
    new_tokens = int(out_ids.shape[1] - in_len)
    # Sharded models may return ids on any stage device — decode from CPU.
    seq = out_ids[0, in_len:].detach().cpu()
    text = tokenizer.decode(seq, skip_special_tokens=True)
    print(f"[zaya-api] generated {new_tokens} tokens in {dt:.1f}s", flush=True)
    return text


@app.get("/health")
@app.get("/v1/health")
def health():
    return {
        "status": "ok",
        "model_dir": MODEL_DIR,
        "load_mode": LOAD_MODE,
        "use_cache": USE_CACHE,
    }


@app.post("/v1/chat/completions")
def chat_completions(req: ChatCompletionRequest):
    if req.stream:
        raise HTTPException(501, "stream=true not supported yet")

    try:
        _load_model()
    except Exception as e:
        raise HTTPException(503, f"model not loaded: {e}") from e

    if hasattr(_tokenizer, "apply_chat_template"):
        prompt = _tokenizer.apply_chat_template(
            [m.model_dump() for m in req.messages],
            tokenize=False,
            add_generation_prompt=True,
        )
    else:
        prompt = "\n".join(f"{m.role}: {m.content}" for m in req.messages) + "\nassistant:"

    max_new = req.max_tokens or DEFAULT_MAX_NEW
    try:
        text = _generate_text(prompt, max_new, req.temperature)
    except torch.cuda.OutOfMemoryError as e:
        if torch.cuda.is_available():
            torch.cuda.empty_cache()
        raise HTTPException(507, f"cuda OOM during generation: {e}") from e
    except Exception as e:
        raise HTTPException(500, str(e)) from e
    now = int(time.time())
    return {
        "id": f"chatcmpl-{uuid.uuid4().hex[:12]}",
        "object": "chat.completion",
        "created": now,
        "model": req.model,
        "choices": [
            {
                "index": 0,
                "message": {"role": "assistant", "content": text},
                "finish_reason": "stop",
            }
        ],
    }


@app.post("/v1/completions")
def completions(req: CompletionRequest):
    max_new = req.max_tokens or DEFAULT_MAX_NEW
    text = _generate_text(req.prompt, max_new, req.temperature)
    now = int(time.time())
    return {
        "id": f"cmpl-{uuid.uuid4().hex[:12]}",
        "object": "text_completion",
        "created": now,
        "model": req.model,
        "choices": [{"index": 0, "text": text, "finish_reason": "stop"}],
    }


def main():
    uvicorn.run(app, host=HOST, port=PORT, log_level="info")


if __name__ == "__main__":
    main()
