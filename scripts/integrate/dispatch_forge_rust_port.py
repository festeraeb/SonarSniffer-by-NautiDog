#!/usr/bin/env python3
"""Dispatch Python→Rust port jobs via Forge (T440) → cesarops2 LLMs."""
from __future__ import annotations

import json
import os
import textwrap
import urllib.error
import urllib.request
from pathlib import Path

# Reuse queue, ordering, skips from direct c2 dispatcher.
from dispatch_c2_rust_port import (  # noqa: E402
    BATCH_ORDER,
    LOG as _UNUSED,
    MAX_BODY,
    MAX_TOKENS,
    OUT_QWEN,
    OUT_RUST,
    PORT_TABLE,
    REPO,
    SKIP_RUST,
    SYSTEM,
    assign_gpu,
    ordered_items,
    resolve_python,
)

FORGE_URL = os.environ.get("FORGE_URL", "http://10.0.0.61:9100").rstrip("/")
QWEN_BASE = os.environ.get("QWEN_URL", "http://10.0.0.201:5200").rstrip("/")
RUST_BASE = os.environ.get("RUST_URL", "http://10.0.0.201:5571").rstrip("/")
LOG = Path(os.environ.get("LOG", "/tmp/forge-rust-port-dispatch.log"))
TIMEOUT = int(os.environ.get("TIMEOUT", "660"))


def log(msg: str) -> None:
    line = f"[forge-rust-port] {msg}"
    print(line, flush=True)
    with LOG.open("a", encoding="utf-8") as f:
        f.write(line + "\n")


def _chatml_prompt(system: str, user: str) -> str:
    """Qwen chat template (matches Forge prompts::format_chatml)."""
    return (
        f"<|im_start|>system\n{system}\n"
        f"<|im_start|>user\n{user}\n"
        f"<|im_start|>assistant\n"
    )


def forge_command(base_url: str, user: str) -> str:
    """Complete on Forge-routed cesarops2 endpoint (/v1/completions).

    Note: Forge /cluster/command currently passes stop=[\"\"] and truncates output;
    we sync routing via Forge then call the routed LLM directly.
    """
    prompt = _chatml_prompt(SYSTEM, user)
    body = {
        "prompt": prompt,
        "max_tokens": MAX_TOKENS,
        "temperature": 0.15,
        "stop": ["</s>", "<|im_start|>"],
        "stream": False,
    }
    url = f"{base_url.rstrip('/')}/v1/completions"
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
        data = json.loads(resp.read().decode())
    if data.get("error"):
        raise RuntimeError(data["error"])
    text = (
        data.get("choices", [{}])[0].get("text")
        or data.get("choices", [{}])[0]
        .get("message", {})
        .get("content", "")
        or ""
    ).strip()
    if not text:
        raise RuntimeError("empty completion")
    return text


def sync_forge_routing() -> None:
    """Point Forge thinker/reviewer at cesarops2 coders."""
    body = {"node": "cesarops2", "action": "sync_llm_endpoints"}
    req = urllib.request.Request(
        f"{FORGE_URL}/cluster/fleet/dispatch",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            out = json.loads(resp.read().decode())
        log(f"sync_llm_endpoints: {out.get('status', out)}")
    except Exception as e:
        log(f"sync_llm_endpoints warn: {e}")


def main() -> None:
    OUT_QWEN.mkdir(parents=True, exist_ok=True)
    OUT_RUST.mkdir(parents=True, exist_ok=True)
    LOG.write_text("", encoding="utf-8")

    sync_forge_routing()

    items = ordered_items()
    results = []
    qwen_n = rust_n = skip_n = 0

    for idx, (item_id, py_rel, rust_rel) in enumerate(items):
        rust_name = Path(rust_rel).name
        if rust_name in SKIP_RUST:
            log(f"SKIP already-ported #{item_id} {rust_name}")
            skip_n += 1
            continue

        py_path = resolve_python(py_rel)
        if py_path is None:
            log(f"SKIP missing source #{item_id} {py_rel}")
            results.append({"id": item_id, "error": "missing python", "py": py_rel})
            continue

        base = Path(py_rel).name.replace(".py", "")
        gpu = assign_gpu(idx)
        out_dir = OUT_RUST if gpu == "rust" else OUT_QWEN
        out_md = out_dir / f"rust__{base}.md"
        endpoint = RUST_BASE if gpu == "rust" else QWEN_BASE

        if out_md.is_file():
            t = out_md.read_text(encoding="utf-8", errors="replace")
            if len(t) > 400 and "## Verdict" in t and "```rust" in t:
                log(f"SKIP done #{item_id} {base} ({gpu})")
                results.append({"id": item_id, "gpu": gpu, "out": str(out_md), "skipped": True})
                continue

        body = py_path.read_text(encoding="utf-8", errors="replace")[:MAX_BODY]
        user = textwrap.dedent(
            f"""
            Convert this merged Python module to Rust for cesarops-inference.
            Python path: {py_path}
            Target Rust path: {REPO / rust_rel}
            Match existing modules under cesarops-inference/src/integrate/ (pub fn API, unit tests, minimal deps).
            --- source ---
            {body}
            """
        ).strip()

        log(f"#{item_id} {base} → {gpu} via forge → {endpoint}")
        try:
            reply = forge_command(endpoint, user)
            if len(reply) < 120 or "## Verdict" not in reply:
                raise ValueError(f"short/invalid reply ({len(reply)} bytes)")
            out_md.write_text(f"# {py_rel}\n\n{reply}\n", encoding="utf-8")
            results.append(
                {
                    "id": item_id,
                    "gpu": gpu,
                    "py": py_rel,
                    "rust": rust_rel,
                    "out": str(out_md),
                    "via": "forge",
                }
            )
            if gpu == "rust":
                rust_n += 1
            else:
                qwen_n += 1
        except Exception as e:
            log(f"ERR #{item_id} {base}: {e}")
            results.append({"id": item_id, "gpu": gpu, "error": str(e), "via": "forge"})

    summary = {
        "forge": FORGE_URL,
        "qwen_endpoint": QWEN_BASE,
        "rust_endpoint": RUST_BASE,
        "qwen_out": str(OUT_QWEN),
        "rust_out": str(OUT_RUST),
        "completed_qwen": qwen_n,
        "completed_rust": rust_n,
        "skipped_existing": skip_n,
        "results": results,
    }
    (OUT_QWEN / "dispatch_summary.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    log(
        f"done qwen={qwen_n} rust={rust_n} skip={skip_n} "
        f"→ {OUT_QWEN}/dispatch_summary.json"
    )


if __name__ == "__main__":
    main()
