#!/usr/bin/env python3
"""TEMPORAL_STACK_LOCAL_SPEC → Forge route → Mixtral thinker → Gemma coder → Mixtral polish."""

from __future__ import annotations

import json
import os
import re
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))
from load_fleet_env import apply_fleet_env, llm_urls  # noqa: E402

REPO = apply_fleet_env(Path(os.environ.get("REPO", Path(__file__).resolve().parents[2])))
SPEC = REPO / "docs/TEMPORAL_STACK_LOCAL_SPEC.md"
OUT = Path(
    os.environ.get(
        "OUT",
        REPO
        / "scripts/role_bench/var/role_bench"
        / f"temporal_stack_fleet_{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}",
    )
)
OUT.mkdir(parents=True, exist_ok=True)

_urls = llm_urls()
FORGE = os.environ.get("FORGE_URL", _urls["forge"]).rstrip("/")
# Default chain: Qwen → Gemma → Qwen (Mixtral optional via THINKER_URL)
THINKER = os.environ.get("THINKER_URL", _urls["qwen"]).rstrip("/")
CODER = os.environ.get("CODER_URL", _urls["gemma"]).rstrip("/")
POLISHER = os.environ.get("POLISHER_URL", _urls["qwen"]).rstrip("/")
TIMEOUT = int(os.environ.get("TEMPORAL_FLEET_TIMEOUT", "1800"))


def log(msg: str) -> None:
    print(msg, flush=True)
    with (OUT / "run.log").open("a") as f:
        f.write(msg + "\n")


def strip_reasoning(text: str) -> str:
    if not text:
        return ""
    text = re.sub(r"\n*<!-- reasoning -->.*", "", text, flags=re.DOTALL)
    return text.strip()


def chat(url: str, system: str, user: str, max_tokens: int = 3072) -> dict:
    t0 = time.time()
    r = requests.post(
        f"{url}/v1/chat/completions",
        json={
            "model": "default",
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0.2,
            "max_tokens": max_tokens,
        },
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    msg = r.json()["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    if not text and msg.get("reasoning_content"):
        text = msg["reasoning_content"]
    text = strip_reasoning(text)
    return {
        "text": text,
        "seconds": round(time.time() - t0, 1),
        "usage": r.json().get("usage"),
        "url": url,
    }


def forge_routing() -> None:
    body = {
        "thinker_endpoint": THINKER,
        "draft_endpoint": THINKER,
        "coder_endpoint": CODER,
        "reviewer_endpoint": CODER,
        "corrector_endpoint": CODER,
        "chat_agent": "gemma",
    }
    try:
        requests.post(
            f"{FORGE}/cluster/routing",
            json=body,
            timeout=30,
        ).raise_for_status()
        (OUT / "forge_routing.json").write_text(json.dumps(body, indent=2))
        log(f"Forge routing → thinker={THINKER} coder={CODER}")
    except Exception as e:
        log(f"Forge routing warn: {e}")


def main() -> None:
    spec = SPEC.read_text()
    (OUT / "spec_source.md").write_text(spec)
    log(f"OUT={OUT}")
    log(f"FORGE={FORGE} THINKER={THINKER} CODER={CODER} POLISHER={POLISHER}")

    forge_routing()

    # Phase 1 — Mixtral thinker
    log(f"Phase 1: thinker plan ({THINKER})")
    thinker_sys = (
        "You are the fleet thinker for cesarops-satellite. "
        "Produce an implementation plan only — no full code dump. Markdown."
    )
    thinker_user = f"""SPEC (TEMPORAL_STACK_LOCAL — one task):

{spec[:12000]}

Deliver:
# Temporal stack local — implementation plan
## 1. Function signature & types for run_temporal_stack_local
## 2. Scene discovery (17 scenes, 3 dirs)
## 3. Temporal cube + clarity ratio + persistence algorithm
## 4. mission.rs wire (stage_temporal_stack branch only)
## 5. Pitfalls (GDAL thread safety, no STAC, no ndarray-npy)
## 6. Acceptance (300m Burns, persistence > 0.3)
Max 900 words."""
    t = chat(THINKER, thinker_sys, thinker_user, max_tokens=2048)
    (OUT / "01_thinker.md").write_text(t.get("text") or f"ERROR")
    (OUT / "01_thinker.json").write_text(json.dumps(t, indent=2))
    log(f"  thinker: {t.get('seconds')}s words={len((t.get('text') or '').split())}")

    plan = t.get("text") or ""

    # Phase 2 — Gemma coder
    log(f"Phase 2: coder ({CODER}) temporal.rs + mission.rs")
    coder_sys = (
        "You are the integrate coder. Write Rust for cesarops-satellite only. "
        "Edit temporal.rs (add run_temporal_stack_local) and show mission.rs patch. "
        "Use existing chip::decode_local_band, find_peak_clusters, cross_reference. "
        "No git push. No STAC/network in local path."
    )
    coder_user = f"""ORIGINAL SPEC:
{spec[:8000]}

THINKER PLAN:
{plan[:6000]}

Deliver:
## temporal.rs — run_temporal_stack_local
(Full function body + any small helpers)

## mission.rs — stage_temporal_stack patch
(Exact rust block for use_local_scenes branch)

## Build / test
```bash
cargo build --release -p cesarops-satellite --features gdal
/data/cargo-target/release/sat-run --spec data/missions/straits_local_run.json ...
```

## Notes
Max 1200 words of code+comments."""
    c = chat(CODER, coder_sys, coder_user, max_tokens=4096)
    (OUT / "02_coder_gemma.md").write_text(c.get("text") or "ERROR")
    (OUT / "02_coder_gemma.json").write_text(json.dumps(c, indent=2))
    log(f"  coder: {c.get('seconds')}s words={len((c.get('text') or '').split())}")

    code = c.get("text") or ""

    # Phase 3 — Mixtral polish / grade
    log(f"Phase 3: polish ({POLISHER})")
    polish_sys = (
        "Lead editor. Merge thinker plan + coder output into one executable handoff. "
        "Flag gaps vs spec. Markdown only."
    )
    polish_user = f"""SPEC EXCERPT:
{spec[:3500]}

THINKER:
{plan[:4000]}

CODER:
{code[:8000]}

Deliver:
# Temporal stack local — merged handoff
## Summary
## Implementation checklist (copy-paste order)
## Code to apply (consolidated)
## Acceptance (Burns 45.87127,-84.58642, 300m, persistence)
## Gaps / must-fix before merge
Max 1000 words."""
    p = chat(POLISHER, polish_sys, polish_user, max_tokens=3072)
    merged = p.get("text") or ""
    (OUT / "MERGED_HANDOFF.md").write_text(merged)
    (OUT / "03_polish.json").write_text(json.dumps(p, indent=2))
    log(f"  polish: {p.get('seconds')}s → MERGED_HANDOFF.md")

    summary = {
        "out": str(OUT),
        "forge": FORGE,
        "thinker": THINKER,
        "coder": CODER,
        "polisher": POLISHER,
        "thinker_s": t.get("seconds"),
        "coder_s": c.get("seconds"),
        "polish_s": p.get("seconds"),
    }
    (OUT / "summary.json").write_text(json.dumps(summary, indent=2))
    log("Done.")


if __name__ == "__main__":
    main()
