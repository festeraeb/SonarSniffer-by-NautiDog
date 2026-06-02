#!/usr/bin/env python3
"""Enhance B-grade integrate modules: add tests + deepen logic (Qwen MoE + Gemma)."""
from __future__ import annotations

import json
import os
import random
import textwrap
import urllib.error
import urllib.request
from pathlib import Path

from dispatch_c2_rust_port import (
    MAX_BODY,
    MAX_TOKENS,
    PORT_TABLE,
    REPO,
    resolve_python,
)

QWEN_URL = os.environ.get("QWEN_URL", "http://10.0.0.201:5200/v1/chat/completions")
GEMMA_URL = os.environ.get("GEMMA_URL", "http://10.0.0.201:5571/v1/chat/completions")
OUT_DIR = Path(os.environ.get("OUT_DIR", REPO / "integrate_out" / "bgrade_enhance"))
LOG = Path(os.environ.get("LOG", "/tmp/bgrade-enhance-dispatch.log"))
TIMEOUT = int(os.environ.get("TIMEOUT", "900"))
SEED = int(os.environ.get("SEED", "4242"))
MAX_ATTEMPTS = int(os.environ.get("MAX_ATTEMPTS", "3"))
MAX_OUT_TOKENS = int(os.environ.get("MAX_OUT_TOKENS", "2048"))

# B-grade, no unit tests (from port_grades.json)
BGRADE_IDS = {
    2, 3, 6, 13, 14, 21, 22, 23, 25, 26, 27, 28, 29, 30, 32, 33, 35, 36,
    40, 41, 42, 43, 44, 48, 50, 51,
}

SYSTEM = """You are a senior CESAROPS Rust engineer improving an existing integrate-layer port.

The file exists but is B-grade: missing unit tests and/or thin logic.

Output markdown ONLY:
## Verdict
KEEP_AND_ENHANCE
## Changes
Bullet list of what you added
## Rust path
Exact path under repo
## Rust source
FULL replacement ```rust ... ``` module with:
- at least 2 #[cfg(test)] mod tests with real assertions (not trivial assert!(true))
- expanded pub fn API matching Python behavior where practical
- serde types, minimal deps, style of cesarops-inference/src/integrate/*.rs
## mod.rs wire
pub mod line if new file
## Risks
Brief bullets
Start with ## Verdict. No chain-of-thought."""


def log(msg: str) -> None:
    line = f"[bgrade-enhance] {msg}"
    print(line, flush=True)
    with LOG.open("a", encoding="utf-8") as f:
        f.write(line + "\n")


def base_url(chat_url: str) -> str:
    chat_url = chat_url.rstrip("/")
    return chat_url[: -len("/v1/chat/completions")] if chat_url.endswith("/v1/chat/completions") else chat_url


def fetch_model_id(chat_url: str) -> str:
    api_url = f"{base_url(chat_url)}/v1/models"
    try:
        with urllib.request.urlopen(api_url, timeout=30) as resp:
            data = json.loads(resp.read().decode())
        models = data.get("data") or []
        if models and isinstance(models[0], dict):
            return models[0].get("id") or "model"
    except Exception:
        pass
    return os.environ.get("FALLBACK_MODEL_ID", "model")


def make_agents() -> list[dict[str, str]]:
    qwen_model = fetch_model_id(QWEN_URL)
    gemma_model = fetch_model_id(GEMMA_URL)
    # Keep named "agents" separate even if they share endpoints.
    return [
        {"name": "thinker", "url": QWEN_URL, "model": qwen_model},
        {"name": "corrector", "url": QWEN_URL, "model": qwen_model},
        {"name": "draft", "url": GEMMA_URL, "model": gemma_model},
        {"name": "reviewer", "url": GEMMA_URL, "model": gemma_model},
    ]


def pick_agent_order(item_id: int, agents: list[dict[str, str]]) -> list[dict[str, str]]:
    rng = random.Random(SEED + item_id)
    order = list(agents)
    rng.shuffle(order)
    return order


def chat(url: str, model_id: str, user: str) -> str:
    body = {
        "model": model_id,
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": user},
        ],
        "max_tokens": min(MAX_TOKENS, MAX_OUT_TOKENS),
        "temperature": 0.12,
        "top_p": 0.9,
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
        data = json.loads(resp.read().decode())
    choice = (data.get("choices") or [{}])[0]
    message = choice.get("message") or {}
    # Handle both /chat/completions and /completions-compatible variants.
    content = message.get("content")
    if content:
        return content
    # llama.cpp can return content in reasoning_content for "thinking" templates
    reasoning = message.get("reasoning_content")
    if reasoning:
        return reasoning
    return choice.get("text") or ""


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    LOG.write_text("", encoding="utf-8")
    results = []
    agents = make_agents()
    log("agents: " + ", ".join(f"{a['name']}={a['model']}@{base_url(a['url'])}" for a in agents))

    for item_id, py_rel, rust_rel in PORT_TABLE:
        if item_id not in BGRADE_IDS:
            continue

        rust_path = REPO / rust_rel
        if not rust_path.is_file():
            log(f"SKIP #{item_id} missing {rust_rel}")
            continue

        py_path = resolve_python(py_rel)
        if py_path is None:
            log(f"SKIP #{item_id} missing python {py_rel}")
            continue

        base = Path(py_rel).name.replace(".py", "")
        out_md = OUT_DIR / f"enhance__{base}.md"

        existing = ""
        if rust_path.is_file():
            existing = rust_path.read_text(encoding="utf-8", errors="replace")[:12000]

        user = textwrap.dedent(
            f"""
            Improve this Rust integrate port (add tests + deepen logic).
            Python: {py_path}
            Target: {rust_path}
            --- existing rust ---
            {existing}
            --- python (truncated) ---
            {py_path.read_text(encoding='utf-8', errors='replace')[:MAX_BODY]}
            """
        ).strip()

        order = pick_agent_order(item_id, agents)
        log(f"#{item_id} {base} agent-order=" + " -> ".join(a["name"] for a in order))
        success = False
        errors: list[str] = []

        for i, agent in enumerate(order[:MAX_ATTEMPTS], start=1):
            try:
                reply = chat(agent["url"], agent["model"], user)
                if len(reply) < 200 or "## Verdict" not in reply or "```rust" not in reply:
                    raise ValueError(f"invalid reply ({len(reply)} bytes)")
                out_md.write_text(f"# enhance {py_rel}\n\n{reply}\n", encoding="utf-8")
                results.append(
                    {
                        "id": item_id,
                        "agent": agent["name"],
                        "model": agent["model"],
                        "attempt": i,
                        "out": str(out_md),
                        "ok": True,
                    }
                )
                success = True
                break
            except Exception as e:
                err = f"{agent['name']} attempt={i}: {e}"
                errors.append(err)
                log(f"ERR #{item_id} {base}: {err}")

        if not success:
            results.append({"id": item_id, "error": " | ".join(errors), "ok": False})

    summary = {"out": str(OUT_DIR), "results": results}
    (OUT_DIR / "enhance_summary.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    log(f"done {len(results)} items → {OUT_DIR}/enhance_summary.json")


if __name__ == "__main__":
    main()
