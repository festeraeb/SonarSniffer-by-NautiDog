#!/usr/bin/env python3
"""A/B Mixtral thinker: RTX hybrid :5200 vs CPU-only :5211."""
from __future__ import annotations

import json
import os
import time
from datetime import datetime, timezone
from pathlib import Path

import requests

REPO = Path(os.environ.get("REPO", Path(__file__).resolve().parents[2]))
OUT = Path(
    os.environ.get(
        "OUT",
        REPO
        / "scripts/role_bench/var/role_bench"
        / f"thinker_ab_{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}",
    )
)
OUT.mkdir(parents=True, exist_ok=True)

LANES = [
    ("RTX-hybrid", os.environ.get("RTX_THINKER_URL", "http://127.0.0.1:5200").rstrip("/")),
    ("CPU-only", os.environ.get("CPU_THINKER_URL", "http://127.0.0.1:5211").rstrip("/")),
]
REVIEWER = os.environ.get("REVIEWER_URL", "http://10.0.0.61:5001").rstrip("/")
TIMEOUT = int(os.environ.get("THINKER_BENCH_TIMEOUT", "600"))

PROMPT = """You are the fleet thinker for Straits satellite+BAG wreck search.

Given: Cedarville steel must calibrate before Burns wood at 45.87127,-84.58642; B02/B03 plume not hull; 300m acceptance.

Deliver ONLY:
## Plan (max 8 bullets)
## Risks (max 3 bullets)
## Next command (one line)"""


def chat(url: str) -> tuple[str, float]:
    t0 = time.time()
    r = requests.post(
        f"{url}/v1/chat/completions",
        json={
            "model": "default",
            "messages": [
                {"role": "system", "content": "Thinker. Markdown only. No preamble."},
                {"role": "user", "content": PROMPT},
            ],
            "temperature": 0.2,
            "max_tokens": 768,
        },
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    text = (r.json()["choices"][0]["message"].get("content") or "").strip()
    return text, round(time.time() - t0, 1)


def grade(label: str, body: str) -> str:
    r = requests.post(
        f"{REVIEWER}/v1/chat/completions",
        json={
            "model": "default",
            "messages": [
                {
                    "role": "system",
                    "content": (
                        "Grade thinker output. Reply ONLY:\n"
                        "## VERDICT: PASS | PARTIAL | FAIL\n"
                        "## Score (0-10)\n## Note (one line)"
                    ),
                },
                {"role": "user", "content": f"LANE: {label}\n\n{body[:6000]}"},
            ],
            "temperature": 0.1,
            "max_tokens": 200,
        },
        timeout=120,
    )
    r.raise_for_status()
    return (r.json()["choices"][0]["message"].get("content") or "").strip()


def main() -> None:
    results = []
    for label, url in LANES:
        rec: dict = {"label": label, "url": url}
        try:
            text, sec = chat(url)
            rec.update(text=text, seconds=sec, words=len(text.split()), error=None)
        except Exception as e:
            rec.update(text="", seconds=0, words=0, error=str(e))
        slug = label.replace("/", "-").lower()
        (OUT / f"{slug}.md").write_text(rec.get("text") or f"ERROR: {rec.get('error')}")
        if rec.get("text"):
            try:
                rec["grade"] = grade(label, rec["text"])
                (OUT / f"{slug}_grade.md").write_text(rec["grade"])
            except Exception as e:
                rec["grade"] = f"grade error: {e}"
        results.append(rec)
        print(f"{label}: {rec.get('seconds')}s words={rec.get('words')} err={rec.get('error')}")
    (OUT / "summary.json").write_text(json.dumps(results, indent=2))
    print(f"OUT={OUT}")


if __name__ == "__main__":
    main()
