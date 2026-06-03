#!/usr/bin/env python3
"""Run Mixtral CPU analysis bench against cesarops-inference wgpu snippets."""
from __future__ import annotations

import json
import pathlib
import sys
import time

import requests

ROOT = pathlib.Path("/data/codebase/repos/wreckhunter2000-1")
BASE = pathlib.Path(__file__).resolve().parent / "var/role_bench"
BASE_URL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:5211"
TIMEOUT = int(sys.argv[2]) if len(sys.argv) > 2 else 3600
MAX_TOKENS = int(sys.argv[6]) if len(sys.argv) > 6 else 4096
OUT = pathlib.Path(sys.argv[3]) if len(sys.argv) > 3 else BASE / "mixtral_cpu_wgpu_analysis.md"
MODEL_ID = sys.argv[4] if len(sys.argv) > 4 else "mixtral"
TITLE = sys.argv[5] if len(sys.argv) > 5 else "Mixtral CPU wgpu analysis"


def snippet(path: str, start: int, end: int) -> str:
    lines = (ROOT / path).read_text(encoding="utf-8", errors="ignore").splitlines()
    part = "\n".join(f"{i + 1}:{lines[i]}" for i in range(start - 1, min(end, len(lines))))
    return f"## {path} {start}-{end}\n{part}\n"


def build_prompt() -> str:
    content = "\n\n".join(
        [
            snippet("cesarops-inference/src/backend_wgpu.rs", 1, 70),
            snippet("cesarops-inference/src/gemma4_gpu_runner.rs", 1, 45),
            snippet("cesarops-inference/src/gpu_context.rs", 1, 70),
            snippet("cesarops-inference/src/pipeline_cache.rs", 1, 70),
        ]
    )
    return (
        "You are a Rust/wgpu performance engineer for legacy NVIDIA GPUs (Pascal/Kepler).\n"
        "Given the snippets, propose speed and correctness improvements for inference.\n"
        "Assume: limited VRAM, shader binding limits, buffer size caps, CPU readback is expensive.\n"
        "Output:\n"
        "- Top 8 bottlenecks ranked by impact\n"
        "- Concrete fixes with file+line refs from snippets\n"
        "- Quick benchmark plan and expected gains\n"
        "- Risks / wrong assumptions to avoid on old GPUs\n"
        "Keep it practical.\n\n"
        + content
    )


def main() -> int:
    prompt = build_prompt()
    payload = {
        "model": MODEL_ID,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.2,
        "max_tokens": MAX_TOKENS,
    }
    t0 = time.time()
    r = requests.post(f"{BASE_URL}/v1/chat/completions", json=payload, timeout=TIMEOUT)
    elapsed = time.time() - t0
    r.raise_for_status()
    data = r.json()
    text = data["choices"][0]["message"]["content"]
    usage = data.get("usage", {})
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        f"# {TITLE}\n\n"
        f"- url: {BASE_URL}\n"
        f"- elapsed_s: {elapsed:.1f}\n"
        f"- usage: {json.dumps(usage)}\n\n"
        f"{text}\n",
        encoding="utf-8",
    )
    print(f"Wrote {OUT} ({elapsed:.1f}s)")
    print(text[:2000])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
