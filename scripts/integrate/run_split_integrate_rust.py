#!/usr/bin/env python3
"""Split laptop-dump integrate across dual P100 + agent-hard skip list."""
from __future__ import annotations

import json
import os
import textwrap
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(os.environ.get("REPO", "/codebase/repos/wreckhunter2000-1"))
OUT = Path(os.environ.get("OUT", REPO / "integrate_out" / "split_rust"))
LOG = Path(os.environ.get("LOG", "/tmp/split_integrate_rust.log"))

# Agent (Cursor) implements these in cesarops-inference/src/integrate/
AGENT_RUST = {
    "cesarops_engine.py",
    "analyze_fuel_leaks.py",
    "lake_michigan_full_scan.py",
    "multi_sensor_scan.py",
    "prioritized_pull_v2.py",
    "daily_satellite_pull.py",
    "cuda_direct.py",
    "integrated_forensic_scan.py",
    "cesarops_cli.py",
    "cesarops_agent_gui.py",
    "full_basin_scan.py",
    "triple_lock_fusion.py",
    "ai_director.py",
    "analyze_crossref.py",
    "drive_identity.py",
    "dynamic_db_key.py",
}

CODER_URL = os.environ.get("P1000_URL", "http://127.0.0.1:5001/v1/chat/completions")
REVIEWER_URL = os.environ.get("P1001_URL", "http://127.0.0.1:5002/v1/chat/completions")

SYSTEM = """You are a senior CESAROPS Rust engineer on the T440 fleet.
Convert the Python laptop-dump script into production Rust for cesarops-inference.

Output markdown ONLY:
## Verdict
ONE of: PORT_TO_PIPELINES | MERGE_INTO_LIVE | ARCHIVE_STUB
## Rust path
e.g. cesarops-inference/src/integrate/<module>.rs
## Rust source
Full ```rust ... ``` module (no placeholders, no todo!, compilable)
## Forge wire
How Forge/pipeline calls this (1-3 bullets)
## Risks
Brief bullets
Start with ## Verdict. No chain-of-thought."""


def chat(url: str, user: str, max_tokens: int = 4096, timeout: int = 120) -> str:
    body = {
        "model": "qwen",
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": user},
        ],
        "max_tokens": max_tokens,
        "temperature": 0.15,
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        data = json.loads(resp.read().decode())
    return data["choices"][0]["message"].get("content") or ""


def log(msg: str) -> None:
    line = f"[split-rust] {msg}"
    print(line, flush=True)
    with LOG.open("a", encoding="utf-8") as f:
        f.write(line + "\n")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "p1000").mkdir(exist_ok=True)
    (OUT / "p1001").mkdir(exist_ok=True)
    manifest = json.loads((REPO / "integrate/_MANIFEST.json").read_text())
    results = []

    for entry in manifest:
        dest = REPO / entry["dest"]
        base = dest.name.replace(".py", "")
        if dest.name in AGENT_RUST:
            log(f"SKIP agent-rust {dest.name}")
            continue
        body = dest.read_text(encoding="utf-8", errors="replace")[:16000]
        user = textwrap.dedent(
            f"""
            Python path: {entry['dest']}
            lines: {entry.get('lines')} stub_score: {entry.get('stub_score')}

            --- source ---
            {body}
            """
        ).strip()
        verdict_hint = entry.get("stub_score", 0)
        use_reviewer = verdict_hint == 0 and entry.get("lines", 0) < 200
        url = REVIEWER_URL if use_reviewer else CODER_URL
        tag = "p1001" if use_reviewer else "p1000"
        out_md = OUT / tag / f"rust__{base}.md"
        if out_md.is_file():
            existing = out_md.read_text(encoding="utf-8", errors="replace")
            if len(existing) > 400 and "## Verdict" in existing:
                log(f"SKIP done {dest.name}")
                results.append({"dest": entry["dest"], "out": str(out_md), "tag": tag, "skipped": True})
                continue
        log(f"{tag} {dest.name}")
        ok = False
        last_err = ""
        for attempt in range(2):
            try:
                reply = chat(url, user, timeout=120)
                if len(reply.strip()) < 80 or "## Verdict" not in reply:
                    last_err = f"short or invalid LLM reply ({len(reply)} bytes)"
                    log(f"retry {dest.name} attempt {attempt+1}/2: {last_err}")
                    continue
                out_md.write_text(f"# {entry['dest']}\n\n{reply}\n", encoding="utf-8")
                results.append({"dest": entry["dest"], "out": str(out_md), "tag": tag})
                ok = True
                break
            except (urllib.error.HTTPError, TimeoutError, OSError, ValueError, json.JSONDecodeError) as e:
                last_err = str(e)
                if attempt < 1:
                    log(f"retry {dest.name} attempt {attempt+1}/2 due to error: {e}")
                    continue
        if not ok:
            log(f"ERR {dest.name}: {last_err}")
            results.append({"dest": entry["dest"], "error": last_err})

    (OUT / "dispatch_summary.json").write_text(
        json.dumps(results, indent=2) + "\n", encoding="utf-8"
    )
    log(f"done {len(results)} tasks → {OUT}")


if __name__ == "__main__":
    main()
