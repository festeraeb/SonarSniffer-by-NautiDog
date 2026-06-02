#!/usr/bin/env python3
"""Dispatch integrate/live_reference work to P100 llama endpoints (:5001 / :5002)."""
from __future__ import annotations

import json
import os
import re
import subprocess
import textwrap
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(os.environ.get("REPO", "/codebase/repos/wreckhunter2000-1"))
OUT = Path(os.environ.get("OUT", REPO / "integrate_out"))
LIVE_PIPE = Path("/codebase/projects/pipelines")
MAX_INTEGRATE = int(os.environ.get("MAX_INTEGRATE_PER_GPU", "40"))

# GPU0 :5001 — integrate queue (Qwen3.6 MoE default)
# GPU1 :5002 — live_reference diffs (optional; same URL if P100_CODER_ONLY=1)
CODER_URL = os.environ.get("P1000_URL", "http://127.0.0.1:5001/v1/chat/completions")
REVIEWER_URL = os.environ.get("P1001_URL", os.environ.get("P1000_URL", "http://127.0.0.1:5002/v1/chat/completions"))
CODER_ONLY = os.environ.get("P100_CODER_ONLY", "0") == "1"
CODER_TAG = os.environ.get("P100_CODER_TAG", "QwenMoE:5001")
REVIEWER_TAG = os.environ.get("P100_REVIEWER_TAG", "QwenMoE:5002" if not CODER_ONLY else "QwenMoE:5001")


def live_path(live_key: str | None) -> Path | None:
    if not live_key:
        return None
    if live_key.startswith("pipelines/"):
        return LIVE_PIPE / live_key.replace("pipelines/", "", 1)
    if live_key.startswith("repo_root/"):
        return REPO / live_key.replace("repo_root/", "", 1)
    return None


def read_tail(p: Path, max_chars: int = 12000) -> str:
    if not p.is_file():
        return ""
    t = p.read_text(encoding="utf-8", errors="replace")
    if len(t) <= max_chars:
        return t
    return t[: max_chars // 2] + "\n\n...[truncated]...\n\n" + t[-max_chars // 2 :]


def unified_diff(a: Path, b: Path, max_lines: int = 200) -> str:
    if not a.is_file() or not b.is_file():
        return "(missing file for diff)"
    r = subprocess.run(
        ["diff", "-u", str(a), str(b)],
        capture_output=True,
        text=True,
    )
    lines = (r.stdout or r.stderr or "").splitlines()
    if len(lines) > max_lines:
        lines = lines[:max_lines] + [f"... ({len(lines) - max_lines} more diff lines)"]
    return "\n".join(lines) if lines else "(no diff — identical)"


def chat(url: str, system: str, user: str, max_tokens: int = 2048) -> str:
    body = {
        "model": "gemma",
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "max_tokens": max_tokens,
        "temperature": 0.2,
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=600) as resp:
            data = json.loads(resp.read().decode())
        return data["choices"][0]["message"]["content"]
    except urllib.error.HTTPError as e:
        return f"HTTP_ERROR {e.code}: {e.read()[:500]}"
    except Exception as e:
        return f"ERROR: {e}"


SYSTEM = """You are a senior CESAROPS engineer on the T440 P100 fleet.
Output markdown with these sections only (no chain-of-thought, no reasoning preamble):
## Verdict
ONE of: KEEP_LIVE | MERGE_INTO_LIVE | PORT_TO_PIPELINES | ARCHIVE_STUB | NEEDS_HUMAN
## Target path
Canonical path under /codebase/projects/pipelines/ or repo root
## Steps
Numbered integration steps (imports, Forge tool wire, tests)
## Risks
Brief bullets
Be concrete. No filler. Start directly with ## Verdict."""


def process_reference(entry: dict) -> dict:
    dest = REPO / entry["dest"]
    lk = entry["live_key"]
    live = live_path(lk)
    diff = unified_diff(live, dest) if live else "(no live file)"
    live_excerpt = read_tail(live, 8000) if live else ""
    ref_excerpt = read_tail(dest, 8000)
    user = textwrap.dedent(
        f"""
        TASK: Compare laptop/dump variant vs LIVE for merge decision.

        live_key: {lk}
        live_path: {live}
        reference_copy: {dest}
        source: {entry.get('source')}

        --- unified diff (live vs reference) ---
        {diff}

        --- live excerpt ---
        {live_excerpt}

        --- reference excerpt ---
        {ref_excerpt}
        """
    ).strip()
    out_dir = OUT / "p1001"
    out_dir.mkdir(parents=True, exist_ok=True)
    base = dest.name.replace(".py", "")
    out_md = out_dir / f"ref__{base}.md"
    print(f"[{REVIEWER_TAG}] reference {lk}", flush=True)
    reply = chat(REVIEWER_URL, SYSTEM, user)
    out_md.write_text(f"# {lk}\n\n{reply}\n", encoding="utf-8")
    return {"kind": "reference", "live_key": lk, "out": str(out_md)}


def process_integrate(entry: dict, shard: int) -> dict:
    """shard 0/1 splits integrate queue across two Gemma batch slots (same URL, serialized)."""
    dest = REPO / entry["dest"]
    lk = entry.get("live_key")
    live = live_path(lk) if lk else None
    body = read_tail(dest, 14000)
    user = textwrap.dedent(
        f"""
        TASK: This file is NOT in live (or unmapped). Decide how to integrate.

        dest: {entry['dest']}
        source: {entry.get('source')}
        lines: {entry.get('lines')} stub_score: {entry.get('stub_score')}
        live_key hint: {lk or 'none'}

        --- full file (may truncate) ---
        {body}
        """
    ).strip()
    out_dir = OUT / ("p1000" if shard == 0 else "p1001")
    out_dir.mkdir(parents=True, exist_ok=True)
    base = Path(entry["dest"]).name.replace(".py", "")
    out_md = out_dir / f"int__{base}.md"
    print(f"[{CODER_TAG}] integrate {entry['dest']}", flush=True)
    reply = chat(CODER_URL, SYSTEM, user)
    out_md.write_text(f"# {entry['dest']}\n\n{reply}\n", encoding="utf-8")
    return {"kind": "integrate", "dest": entry["dest"], "out": str(out_md)}


def main() -> None:
    ref_manifest = json.loads((REPO / "live_reference/_MANIFEST.json").read_text())
    int_manifest = json.loads((REPO / "integrate/_MANIFEST.json").read_text())

    results = []
    # live_reference → Qwen :5002 (all 7)
    for entry in ref_manifest:
        results.append(process_reference(entry))

    # integrate → Gemma :5001 (split output dirs for tracking, same model)
    half = MAX_INTEGRATE
    for i, entry in enumerate(int_manifest[: half * 2]):
        results.append(process_integrate(entry, i % 2))

    summary = OUT / "dispatch_summary.json"
    summary.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {summary} ({len(results)} tasks)", flush=True)


if __name__ == "__main__":
    main()
