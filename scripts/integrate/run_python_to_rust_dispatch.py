#!/usr/bin/env python3
"""Dual-P100 parallel Rust conversion for merged Python pipeline files."""
from __future__ import annotations

import json
import os
import textwrap
import threading
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

REPO = Path(os.environ.get("REPO", "/codebase/repos/wreckhunter2000-1"))
QUEUE = REPO / "integrate" / "PYTHON_TO_RUST_QUEUE.json"
OUT = Path(os.environ.get("OUT", REPO / "integrate_out" / "split_rust"))
LOG = Path(os.environ.get("LOG", "/tmp/python_to_rust_dispatch.log"))

P1000_URL = os.environ.get("P1000_URL", "http://127.0.0.1:5001/v1/chat/completions")
P1001_URL = os.environ.get("P1001_URL", "http://127.0.0.1:5002/v1/chat/completions")
MAX_SOURCE = int(os.environ.get("MAX_SOURCE", "12000"))
MAX_TOKENS = int(os.environ.get("MAX_TOKENS", "8192"))
TIMEOUT = int(os.environ.get("TIMEOUT", "300"))
WORKERS = int(os.environ.get("WORKERS", "2"))

SYSTEM = """You are a senior CESAROPS Rust engineer on the T440 fleet.
Convert the merged Python pipeline script into production Rust for cesarops-inference.

Output markdown ONLY:
## Verdict
PORT_TO_PIPELINES
## Rust path
exact path given in the user message
## Rust source
Full ```rust ... ``` module (no placeholders, no todo!, compilable)
## Forge wire
How Forge/pipeline calls this (1-3 bullets)
## Risks
Brief bullets
Start with ## Verdict. No chain-of-thought."""

_log_lock = threading.Lock()


def log(msg: str) -> None:
    line = f"[py2rust] {msg}"
    with _log_lock:
        print(line, flush=True)
        with LOG.open("a", encoding="utf-8") as f:
            f.write(line + "\n")


def chat(url: str, user: str) -> str:
    body = {
        "model": "qwen",
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": user},
        ],
        "max_tokens": MAX_TOKENS,
        "temperature": 0.15,
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
        data = json.loads(resp.read().decode())
    return data["choices"][0]["message"].get("content") or ""


def plan_ok(path: Path) -> bool:
    if not path.is_file():
        return False
    text = path.read_text(encoding="utf-8", errors="replace")
    return len(text) > 400 and "## Verdict" in text and "```rust" in text


def existing_plan(base: str) -> Path | None:
    for tag in ("p1000", "p1001"):
        p = OUT / tag / f"rust__{base}.md"
        if plan_ok(p):
            return p
    return None


def dispatch_one(entry: dict, url: str, tag: str) -> dict:
    base = entry["source_py"].replace(".py", "")
    out_md = OUT / tag / f"rust__{base}.md"
    py_path = Path(entry["python_path"])
    if not py_path.is_file():
        return {"source": entry["source_py"], "error": f"missing {py_path}"}

    if plan_ok(out_md):
        return {"source": entry["source_py"], "out": str(out_md), "skipped": True}

    body = py_path.read_text(encoding="utf-8", errors="replace")[:MAX_SOURCE]
    user = textwrap.dedent(
        f"""
        Convert this merged Python module to Rust for cesarops-inference.

        Python path: {entry['python_path']}
        Target Rust path: {entry['rust_path']}

        Match existing integrate modules (pub fn API, unit tests, minimal deps).

        --- source ---
        {body}
        """
    ).strip()

    last_err = ""
    for attempt in range(3):
        try:
            log(f"{tag} {entry['source_py']} attempt {attempt + 1}/3")
            reply = chat(url, user)
            if len(reply.strip()) < 120 or "## Verdict" not in reply or "```rust" not in reply:
                last_err = f"invalid reply ({len(reply)} bytes)"
                continue
            out_md.parent.mkdir(parents=True, exist_ok=True)
            out_md.write_text(f"# {entry['python_path']}\n\n{reply}\n", encoding="utf-8")
            return {"source": entry["source_py"], "out": str(out_md), "tag": tag}
        except (urllib.error.HTTPError, TimeoutError, OSError, ValueError, json.JSONDecodeError) as e:
            last_err = str(e)
    return {"source": entry["source_py"], "error": last_err, "tag": tag}


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "p1000").mkdir(exist_ok=True)
    (OUT / "p1001").mkdir(exist_ok=True)

    queue = json.loads(QUEUE.read_text(encoding="utf-8"))
    todo = []
    for entry in queue:
        base = entry["source_py"].replace(".py", "")
        if existing_plan(base):
            log(f"SKIP done {entry['source_py']}")
            continue
        todo.append(entry)

    log(f"dispatching {len(todo)} / {len(queue)} on {WORKERS} workers (timeout={TIMEOUT}s)")
    urls = [P1000_URL, P1001_URL]
    tags = ["p1000", "p1001"]
    results = []

    with ThreadPoolExecutor(max_workers=WORKERS) as pool:
        futures = []
        for i, entry in enumerate(todo):
            futures.append(
                pool.submit(dispatch_one, entry, urls[i % 2], tags[i % 2])
            )
        for fut in as_completed(futures):
            row = fut.result()
            results.append(row)
            if row.get("error"):
                log(f"ERR {row['source']}: {row['error']}")
            elif row.get("skipped"):
                pass
            else:
                log(f"OK {row['source']} -> {row.get('out')}")

    summary = OUT / "python_to_rust_summary.json"
    summary.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    ok = sum(1 for r in results if r.get("out") and not r.get("skipped"))
    err = sum(1 for r in results if r.get("error"))
    log(f"finished ok={ok} err={err} skipped={len(queue)-len(todo)}")

if __name__ == "__main__":
    main()
