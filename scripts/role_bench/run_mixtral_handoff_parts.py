#!/usr/bin/env python3
"""Run unfinished satellite handoff parts on Mixtral with short, split prompts."""

from __future__ import annotations

import json
import os
import re
import sys
import time

# sys used above for path
from datetime import datetime, timezone
from pathlib import Path

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))
from load_fleet_env import apply_fleet_env, llm_urls  # noqa: E402

REPO = apply_fleet_env(Path(os.environ.get("REPO", Path(__file__).resolve().parents[2])))
_urls = llm_urls()
HANDOFF = REPO / "docs/SATELLITE_PIPELINE_HANDOFF.md"
PRIOR_OUT = Path(os.environ.get("PRIOR_OUT", ""))
OUT = Path(
    os.environ.get(
        "OUT",
        REPO
        / "scripts/role_bench/var/role_bench"
        / f"mixtral_parts_{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}",
    )
)
OUT.mkdir(parents=True, exist_ok=True)

# Use MIXTRAL_URL explicitly — do not inherit CPU_URL (often points at Gemma in fleet env).
MIXTRAL = _urls["mixtral"]
REVIEWER = os.environ.get("REVIEWER_URL", _urls["gemma"]).rstrip("/")
TIMEOUT = int(os.environ.get("MIXTRAL_CHAT_TIMEOUT", "2400"))

# Curated context — avoids dumping the full handoff into Mixtral.
HANDOFF_CORE = """\
Satellite pipeline handoff (cesarops-satellite, offline Sentinel-2):
- Goal: flag optical candidate within ~300m of BAG masked wreck at 45.87127°N, -84.58642°W
  (Robert Burns hypothesis: ~126ft wood, ~32ft relief; signal = water-column plume, not hull).
- Calibration: Cedarville steel 45.7873°N, -84.6708°W MUST pass before trusting Burns.
- Controls: Eber Ward, William Young, M. Stalker, Elva (known_wrecks_straits.json).
- Bands: B02 blue + B03 green (column); B04/B08 surface only. Depth ~34m — no bottom reflectance.
- Wood vs steel: down-weight thermal for Burns; zebra-clarity + glint for wood; thermal for steel.
- Data: data/straits_optical_clear|2022|2023/sentinel2_aws (local GeoTIFFs). CPU: rayon on 32 cores.
- Optional: 2× P100 via cudarc if CPU throughput insufficient. Do not edit cesarops-satellite on T440 path.
"""

MICRO_PARTS = [
    {
        "id": "mixtral_d1_gpu",
        "section": "## GPU optional plan (go/no-go criteria)",
        "system": (
            "You are a systems engineer. Answer in markdown only. "
            "No chain-of-thought, no preamble."
        ),
        "user": f"""{HANDOFF_CORE}

Task: STEP 5 optional GPU — when is rayon on 32 Xeon cores enough vs cudarc on P100?

Deliver ONLY this section (max 200 words):
## GPU optional plan (go/no-go criteria)
Include explicit GO and NO-GO bullets and Cedarville-first rule.""",
        "max_tokens": 512,
    },
    {
        "id": "mixtral_d2_cross_sensor",
        "section": "## Cross-sensor confirmation story",
        "system": "Science writer. Markdown only. No reasoning preamble.",
        "user": f"""{HANDOFF_CORE}

Task: Explain cross-sensor confirmation — BAG bathymetric relief vs Sentinel-2 optical plume.

Deliver ONLY (max 200 words):
## Cross-sensor confirmation story
Cover: BAG hard detection, S2 column signal, 300m tolerance rationale, what confirms Burns.""",
        "max_tokens": 512,
    },
    {
        "id": "mixtral_d3_exec",
        "section": "## Executive summary",
        "system": "Technical lead. Markdown only. No preamble.",
        "user": f"""{HANDOFF_CORE}

Deliver ONLY (max 150 words):
## Executive summary
Exactly 5 bullet points: objective, calibration gate, physics, execution path, success criterion.""",
        "max_tokens": 400,
    },
    {
        "id": "mixtral_ops_commands",
        "section": "## Commands (copy-paste)",
        "system": "Ops planner. Markdown only. No preamble.",
        "user": f"""{HANDOFF_CORE}

Additional: binary sat-run at /data/cargo-target/release/sat-run; mission straits_local_run.json;
build: cargo build --release -p cesarops-satellite --features gdal

Deliver ONLY these two sections (max 250 words total):
## Mission JSON edits (bbox, gt_wreck_names)
## Commands (copy-paste)
Cedarville calibration run first, then straits_local_run.json.""",
        "max_tokens": 700,
    },
    {
        "id": "mixtral_ops_acceptance",
        "section": "## Acceptance checklist",
        "system": "QA lead. Markdown only. No preamble.",
        "user": f"""{HANDOFF_CORE}

Deliver ONLY (max 200 words):
## Acceptance checklist
## Expected report artifacts
Include 300m rule and Cedarville must-pass gate.""",
        "max_tokens": 512,
    },
    {
        "id": "mixtral_science_calibration",
        "section": "## Calibration order",
        "system": "Scientific reviewer. Markdown only. No preamble.",
        "user": f"""{HANDOFF_CORE}

Deliver ONLY (max 200 words):
## Calibration order (Cedarville → controls → Burns AOI)
## False confidence guards (max 5 bullets)""",
        "max_tokens": 600,
    },
    {
        "id": "mixtral_science_weights",
        "section": "## Concept weights",
        "system": "Scientific reviewer. Markdown only. No preamble.",
        "user": f"""{HANDOFF_CORE}

Deliver ONLY (max 250 words):
## Concept weights (table: concept × wreck type)
Use a markdown table (concept | steel | wood | bands).

## What would falsify a Burns hit (max 5 bullets)""",
        "max_tokens": 700,
    },
]

GRADE_SYSTEM = (
    "You grade a model shard for a satellite wreck pipeline handoff. "
    "Reply with ONLY these markdown sections — no reasoning, no XML, no preamble:\n"
    "## VERDICT: PASS | PARTIAL | FAIL\n"
    "## Score (0-10)\n"
    "## Strengths\n"
    "## Gaps / must-fix\n"
    "## Merge notes"
)

GRADE_TEMPLATE = """SHARD: {shard_id}
REQUIREMENTS: {requirements}

MODEL OUTPUT:
{body}

Under 300 words total."""


def log(msg: str) -> None:
    print(msg, flush=True)
    with (OUT / "run.log").open("a") as f:
        f.write(msg + "\n")


def strip_reasoning(text: str) -> str:
    if not text:
        return ""
    text = re.sub(r"\n*<!-- reasoning -->.*", "", text, flags=re.DOTALL)
    text = re.sub(r"^Here's a thinking process:.*?(?=^## )", "", text, flags=re.DOTALL | re.M)
    return text.strip()


def extract_grade_sections(text: str) -> str:
    """Pull rubric sections out of Qwen-style reasoning blobs."""
    m = re.search(
        r"(## VERDICT:.*)",
        text,
        flags=re.DOTALL | re.I,
    )
    return m.group(1).strip() if m else ""


def chat(url: str, system: str, user: str, max_tokens: int = 512) -> dict:
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
    d = r.json()
    msg = d["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    if not text and msg.get("reasoning_content"):
        text = extract_grade_sections(msg["reasoning_content"])
    text = strip_reasoning(text)
    return {
        "text": text,
        "seconds": round(time.time() - t0, 1),
        "usage": d.get("usage"),
        "url": url,
    }


def run_part(part: dict) -> dict:
    log(f"  part {part['id']} …")
    try:
        out = chat(MIXTRAL, part["system"], part["user"], part["max_tokens"])
        rec = {**part, **out, "error": None}
    except Exception as e:
        rec = {**part, "text": "", "seconds": 0, "error": str(e)}
    (OUT / f"{part['id']}.md").write_text(rec.get("text") or f"ERROR: {rec.get('error')}")
    (OUT / f"{part['id']}.json").write_text(
        json.dumps({k: v for k, v in rec.items() if k not in ("system", "user")}, indent=2)
    )
    log(f"    {part['id']}: {rec.get('seconds', 0)}s err={rec.get('error')} words={len((rec.get('text') or '').split())}")
    return rec


def grade(shard_id: str, body: str, requirements: str) -> dict:
    body = strip_reasoning(body)[:8000]
    if not body.strip() or body.startswith("ERROR:"):
        text = "## VERDICT: FAIL\n## Score (0-10)\n0\n## Strengths\n(none)\n## Gaps / must-fix\nEmpty output.\n## Merge notes\nRe-run part."
        (OUT / f"{shard_id}_grade.md").write_text(text)
        return {"text": text}
    prompt = GRADE_TEMPLATE.format(shard_id=shard_id, requirements=requirements, body=body)
    try:
        g = chat(REVIEWER, GRADE_SYSTEM, prompt, max_tokens=600)
        (OUT / f"{shard_id}_grade.md").write_text(g["text"])
        return g
    except Exception as e:
        err = f"## VERDICT: FAIL\n## Score (0-10)\n0\n## Gaps / must-fix\nGRADE ERROR: {e}"
        (OUT / f"{shard_id}_grade.md").write_text(err)
        return {"text": err, "error": str(e)}


def stitch_synthesis(parts: list[dict]) -> str:
    order = ["mixtral_d1_gpu", "mixtral_d2_cross_sensor", "mixtral_d3_exec"]
    blocks = []
    for pid in order:
        p = next(x for x in parts if x["id"] == pid)
        blocks.append(p.get("text") or "")
    return "\n\n".join(blocks).strip()


def stitch_ops(parts: list[dict]) -> str:
    ids = ["mixtral_ops_commands", "mixtral_ops_acceptance"]
    return "\n\n".join(next(x for x in parts if x["id"] == i).get("text") or "" for i in ids).strip()


def stitch_science(parts: list[dict]) -> str:
    ids = ["mixtral_science_calibration", "mixtral_science_weights"]
    return "\n\n".join(next(x for x in parts if x["id"] == i).get("text") or "" for i in ids).strip()


def parse_verdict(grade_text: str) -> tuple[str, str]:
    m = re.search(r"## VERDICT:\s*(PASS|PARTIAL|FAIL)", grade_text, re.I)
    s = re.search(r"## Score.*?\n\s*([0-9]+(?:\.[0-9]+)?)", grade_text, re.I)
    return (m.group(1).upper() if m else "?"), (s.group(1) if s else "?")


def main() -> None:
    (OUT / "handoff_core.txt").write_text(HANDOFF_CORE)
    if HANDOFF.exists():
        (OUT / "handoff_source.md").write_text(HANDOFF.read_text())

    log(f"OUT={OUT} MIXTRAL={MIXTRAL} TIMEOUT={TIMEOUT}s parts={len(MICRO_PARTS)}")

    results: list[dict] = []
    for part in MICRO_PARTS:
        results.append(run_part(part))
        time.sleep(0.5)  # avoid hammering llama-server

    synth = stitch_synthesis(results)
    ops = stitch_ops(results)
    science = stitch_science(results)

    (OUT / "shard_d_cpu_synthesis.md").write_text(synth)
    (OUT / "shard_d_cpu_synthesis.json").write_text(
        json.dumps({"label": "CPU-Mixtral", "url": MIXTRAL, "parts": [p["id"] for p in results[:3]]}, indent=2)
    )
    (OUT / "mixtral_ops_combined.md").write_text(ops)
    (OUT / "mixtral_science_combined.md").write_text(science)

    log("Phase 2: grade stitched bundles (Qwen reviewer)")
    g_synth = grade(
        "shard_d_cpu_synthesis_mixtral",
        synth,
        "GPU go/no-go, cross-sensor BAG+S2 story, 5-bullet executive summary; Cedarville-first.",
    )
    g_ops = grade(
        "mixtral_ops_combined",
        ops,
        "Mission JSON edits, sat-run commands, acceptance checklist, report artifacts, 300m rule.",
    )
    g_sci = grade(
        "mixtral_science_combined",
        science,
        "Calibration order, concept weight table, false confidence guards, Burns falsifiers.",
    )

  # Also grade each micro-part
    part_grades = []
    for p in results:
        req = p.get("section", p["id"])
        g = grade(f"{p['id']}", p.get("text") or "", req)
        v, sc = parse_verdict(g.get("text", ""))
        part_grades.append({"id": p["id"], "verdict": v, "score": sc, "seconds": p.get("seconds"), "error": p.get("error")})

    summary = {
        "out": str(OUT),
        "mixtral_url": MIXTRAL,
        "timeout_sec": TIMEOUT,
        "stitched": {
            "synthesis": parse_verdict(g_synth.get("text", "")),
            "ops": parse_verdict(g_ops.get("text", "")),
            "science": parse_verdict(g_sci.get("text", "")),
        },
        "micro_parts": part_grades,
    }
    (OUT / "summary.json").write_text(json.dumps(summary, indent=2))

    if PRIOR_OUT and Path(PRIOR_OUT).is_dir():
        prior = Path(PRIOR_OUT)
        (prior / "shard_d_cpu_synthesis.md").write_text(synth)
        (prior / "shard_d_cpu_synthesis_grade.md").write_text(g_synth.get("text", ""))
        (prior / "mixtral_ops_combined.md").write_text(ops)
        (prior / "mixtral_science_combined.md").write_text(science)
        log(f"Updated prior run: {prior}")

    log("Done.")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
