#!/usr/bin/env python3
"""Shard SATELLITE_PIPELINE_HANDOFF.md across fleet LLMs, grade, polish."""

from __future__ import annotations

import json
import os
import re
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from pathlib import Path

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))
from load_fleet_env import apply_fleet_env, llm_urls  # noqa: E402

REPO = apply_fleet_env(Path(os.environ.get("REPO", Path(__file__).resolve().parents[2])))
_urls = llm_urls()
HANDOFF = REPO / "docs/SATELLITE_PIPELINE_HANDOFF.md"
OUT = Path(
    os.environ.get(
        "OUT",
        REPO / "scripts/role_bench/var/role_bench" / f"satellite_handoff_{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}",
    )
)
OUT.mkdir(parents=True, exist_ok=True)

GEMMA = _urls["gemma"]
QWEN = _urls["qwen"]
RTX = _urls["rtx"]
CPU = _urls["mixtral"]
REVIEWER = os.environ.get("REVIEWER_URL", QWEN).rstrip("/")
# RTX (:5200) and Qwen (:5002) often have 3k–4k ctx; Gemma (:5001) fits merge prompts.
POLISHER = os.environ.get("POLISHER_URL", _urls["polisher"] or GEMMA).rstrip("/")
CPU_HANDOFF_CHARS = int(os.environ.get("CPU_HANDOFF_CHARS", "2500"))
MIXTRAL_TIMEOUT = int(os.environ.get("MIXTRAL_CHAT_TIMEOUT", "2400"))

TIMEOUT = int(os.environ.get("HANDOFF_CHAT_TIMEOUT", "900"))

SHARDS = [
    {
        "id": "shard_a_gemma_impl",
        "label": "P100-Gemma",
        "url": GEMMA,
        "role": "integrate_coder",
        "system": (
            "You are the integrate coder for cesarops-satellite. "
            "Produce concrete Rust implementation guidance only. No git push."
        ),
        "task": """From the handoff, own STEPS 1–2 only:
- Build with `--features gdal`; fix `decode_local_band` / `bbox_to_pixel_window` API (gdal 0.17).
- Design `run_poc_aoi_local` in poc.rs with rayon `par_iter` over scenes.
- Wire `mission.rs::stage_poc_aoi` when `use_local_scenes` + `download_dir`.

Deliver:
## Files to touch (paths)
## Code changes (bullet steps with function names)
## Compile / test commands
## Pitfalls (max 5)
Keep under 700 words. Do NOT implement Burns interpretation yet.""",
    },
    {
        "id": "shard_b_qwen_science",
        "label": "P100-Qwen",
        "url": QWEN,
        "role": "science_reviewer",
        "system": (
            "You are the scientific reviewer for Straits wreck satellite search. "
            "Emphasize calibration discipline and physics."
        ),
        "task": """From the handoff, own detection physics + calibration:
- Cedarville MUST pass before trusting Burns / masked target.
- Wood (Burns) vs steel (Cedarville): band/concept weighting.
- Blue/green column signal vs bottom reflectance; glint as signal.

Deliver:
## Calibration order (Cedarville → controls → Burns AOI)
## Concept weights (table: concept × wreck type)
## False confidence guards
## What would falsify a Burns hit
Under 600 words.""",
    },
    {
        "id": "shard_c_rtx_ops",
        "label": "RTX-2060",
        "url": RTX,
        "role": "ops_planner",
        "system": "You are the ops planner. Exact shell commands and acceptance checks.",
        "task": """From the handoff, own STEPS 3–4 + ACCEPTANCE:
- `sat-run` commands for Cedarville bbox spec then `straits_local_run.json`.
- Data paths under `data/straits_optical_*`.
- Report fields to inspect; 300m distance rule.

Deliver:
## Mission JSON edits (bbox, gt_wreck_names)
## Commands (copy-paste)
## Acceptance checklist
## Expected report artifacts
Under 600 words.""",
    },
    {
        "id": "shard_d_cpu_synthesis",
        "label": "CPU-Mixtral",
        "url": CPU,
        "role": "synthesizer",
        "system": "You synthesize risks and optional GPU path. Be concise.",
        "task": """From the handoff, own STEP 5 (optional GPU) + cross-sensor narrative:
- When rayon on 32 Xeon cores is enough vs cudarc on P100.
- Cross-sensor confirmation: BAG masked wreck vs S2 optical plume.
- Headline result framing if candidate near 45.87127,-84.58642.

Deliver:
## GPU optional plan (go/no-go criteria)
## Cross-sensor confirmation story
## Executive summary (5 bullets)
Under 500 words.""",
    },
]

GRADE_SYSTEM = (
    "You grade another model's satellite pipeline shard. Be strict but fair. "
    "Output exactly these sections."
)
GRADE_TEMPLATE = """SHARD: {shard_id} ({role})
MODEL OUTPUT:
{body}

Grade against the handoff requirements. Deliver:
## VERDICT: PASS | PARTIAL | FAIL
## Score (0-10)
## Strengths
## Gaps / must-fix
## Merge notes (what the final plan must include)
Under 350 words."""


def log(msg: str) -> None:
    print(msg, flush=True)
    with (OUT / "run.log").open("a") as f:
        f.write(msg + "\n")


def chat(url: str, system: str, user: str, max_tokens: int = 2048, timeout: int | None = None) -> dict:
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
        timeout=timeout if timeout is not None else TIMEOUT,
    )
    r.raise_for_status()
    d = r.json()
    msg = d["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    if msg.get("reasoning_content") and url.rstrip("/") != CPU.rstrip("/"):
        text += "\n\n<!-- reasoning -->\n" + msg["reasoning_content"]
    return {
        "text": text,
        "seconds": round(time.time() - t0, 1),
        "usage": d.get("usage"),
        "url": url,
    }


def run_shard(shard: dict, handoff: str) -> dict:
    body = handoff
    if shard["id"] == "shard_d_cpu_synthesis" and len(handoff) > CPU_HANDOFF_CHARS:
        body = (
            handoff[:CPU_HANDOFF_CHARS]
            + "\n\n[... handoff truncated for CPU context; see repo docs/SATELLITE_PIPELINE_HANDOFF.md ...]\n"
        )
    user = f"HANDOFF (full context):\n\n{body}\n\n---\n\n{shard['task']}"
    req_timeout = MIXTRAL_TIMEOUT if shard["url"].rstrip("/") == CPU.rstrip("/") else None
    try:
        out = chat(shard["url"], shard["system"], user, timeout=req_timeout)
        rec = {**shard, **out, "error": None}
    except Exception as e:
        rec = {**shard, "text": "", "seconds": 0, "error": str(e)}
    slug = shard["id"]
    (OUT / f"{slug}.md").write_text(rec.get("text") or f"ERROR: {rec.get('error')}")
    (OUT / f"{slug}.json").write_text(json.dumps({k: v for k, v in rec.items() if k != "task"}, indent=2))
    log(f"  shard {shard['label']}: {rec.get('seconds', 0)}s err={rec.get('error')}")
    return rec


def grade_shard(shard: dict, body: str) -> dict:
    if not body.strip():
        return {"text": "VERDICT: FAIL\nNo output.", "error": "empty"}
    prompt = GRADE_TEMPLATE.format(
        shard_id=shard["id"], role=shard["role"], body=body[:12000]
    )
    try:
        g = chat(REVIEWER, GRADE_SYSTEM, prompt, max_tokens=1024)
        slug = shard["id"]
        (OUT / f"{slug}_grade.md").write_text(g["text"])
        return g
    except Exception as e:
        return {"text": f"GRADE ERROR: {e}", "error": str(e)}


def polish_merge(handoff: str, shards: list[dict], grades: list[dict]) -> str:
    parts = []
    for s, g in zip(shards, grades):
        parts.append(
            f"### {s['label']} ({s['role']})\n{(s.get('text') or '')[:2000]}\n\n"
            f"**Review:**\n{(g.get('text') or '')[:500]}\n"
        )
    bundle = "\n---\n".join(parts)[:8000]
    sys_msg = (
        "You are the lead editor. Merge shard outputs into one executable plan. "
        "Preserve Cedarville-first calibration. No contradictions. Markdown."
    )
    user = f"""ORIGINAL HANDOFF:
{handoff[:3500]}

SHARD OUTPUTS + GRADES:
{bundle}

Deliver one document:
# Satellite pipeline — merged execution plan
## 1. Build & wire (gdal + rayon)
## 2. Science & calibration
## 3. Run order & commands
## 4. Acceptance & Burns check
## 5. Optional GPU
## 6. Risks & do-not-trust-until
Max 1200 words."""
    p = chat(POLISHER, sys_msg, user, max_tokens=3072)
    return p["text"]


def main() -> None:
    handoff = HANDOFF.read_text()
    (OUT / "handoff_source.md").write_text(handoff)
    log(f"OUT={OUT}")
    log("Phase 1: parallel shards")
    results: list[dict] = []
    with ThreadPoolExecutor(max_workers=4) as ex:
        futs = {ex.submit(run_shard, s, handoff): s for s in SHARDS}
        for fut in as_completed(futs):
            results.append(fut.result())
    results.sort(key=lambda r: r["id"])

    log("Phase 2: grade each shard (reviewer)")
    grades = [grade_shard(s, s.get("text") or "") for s in results]

    log("Phase 3: polish merge")
    merged = polish_merge(handoff, results, grades)
    (OUT / "MERGED_PLAN.md").write_text(merged)

    summary = {
        "out": str(OUT),
        "shards": [
            {
                "id": s["id"],
                "label": s["label"],
                "seconds": s.get("seconds"),
                "error": s.get("error"),
                "words": len((s.get("text") or "").split()),
            }
            for s in results
        ],
    }
    (OUT / "summary.json").write_text(json.dumps(summary, indent=2))
    log(f"Done → {OUT}/MERGED_PLAN.md")


if __name__ == "__main__":
    main()
