#!/usr/bin/env python3
"""
Read gemini_proposal.json → write cursor_response.json with repo-grounded pushback.

Usage:
  python3 scripts/forge_collab/process_collab_inbox.py
  python3 scripts/forge_collab/process_collab_inbox.py --proposal var/forge_collab/inbox/gemini_proposal.json
"""
from __future__ import annotations

import argparse
import json
import re
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
COLLAB = REPO / "var" / "forge_collab"
INBOX = COLLAB / "inbox"
OUTBOX = COLLAB / "outbox"

# Repo anchors Cursor enforces (Gemini often gets these wrong)
REAL_KNOBS = "Knobs in cesarops-satellite/src/types.rs (poc_zscore_threshold, poc_max_candidates, poc_min_separation_px, poc_downsample_max_dim)"
REAL_RUN = "/data/cargo-target/release/sat-run --spec data/missions/straits_local_run.json --root /data/cesarops/satellite_data"
FAKE_PATTERNS = [
    r"knobs\.pon",
    r"cesarops-inference",
    r"cargo run -- clear-water",
    r"numpy",
    r"np\.clip",
]


def load_json(path: Path) -> dict:
    if not path.is_file():
        raise FileNotFoundError(path)
    return json.loads(path.read_text())


def find_pushbacks(proposal: dict) -> list[dict]:
    issues: list[dict] = []
    scrubbed = {k: v for k, v in proposal.items() if k != "do_not_invent"}
    blob = json.dumps(scrubbed).lower()
    for pat in FAKE_PATTERNS:
        if re.search(pat, blob, re.I):
            issues.append(
                {
                    "code": "invented_api",
                    "pattern": pat,
                    "cursor_fix": f"Use {REAL_KNOBS} and {REAL_RUN}",
                }
            )
    physics = proposal.get("physics") or {}
    files = physics.get("files") or []
    for f in files:
        if f.startswith("src/") and not (REPO / "cesarops-satellite" / f).exists() and f != "src/phase_corr.rs":
            # phase_corr may exist now
            full = REPO / "cesarops-satellite" / f
            if not full.exists():
                issues.append(
                    {
                        "code": "missing_file",
                        "file": f,
                        "cursor_fix": "Create only if in plan; else redirect to temporal.rs / poc.rs",
                    }
                )
    return issues


def build_response(proposal: dict, pushbacks: list[dict]) -> dict:
    status = "pushback" if pushbacks else "accept_with_notes"
    lane = (proposal.get("forge") or {}).get("lane", "A")
    route = "1" if lane in ("A", "both", "lane_a") else "2"

    implementation = []
    physics = proposal.get("physics") or {}
    for ch in physics.get("changes") or []:
        implementation.append({"item": ch, "owner": "cursor", "verify": "cargo test + sat-run"})

    # Known shipped baseline Cursor can affirm
    affirmations = []
    if "phase" in json.dumps(proposal).lower() or "correlation" in json.dumps(proposal).lower():
        affirmations.append(
            "phase_corr.rs + temporal align_clarity_stack_phase_corr already in repo — measure before re-implementing"
        )
    if "z" in json.dumps(proposal).lower() or "clarity" in json.dumps(proposal).lower():
        affirmations.append("poc.rs BLUE_GREEN_Z_CAP=4.0 + edge margin 3 shipped")

    return {
        "round_id": proposal.get("round_id", "unknown"),
        "from": "cursor",
        "ts": datetime.now(timezone.utc).isoformat(),
        "status": status,
        "pushbacks": pushbacks,
        "affirmations": affirmations,
        "accepted_hypothesis": proposal.get("hypothesis", ""),
        "implementation_plan": implementation,
        "forge_action": {
            "route": route,
            "job_file": "scripts/role_bench/dual_lane_job_03_phase_correlation.json",
            "prompt_profile": "tight",
            "run_after_code": REAL_RUN,
        },
        "ask_gemini_to_confirm": [
            "Reply in inbox/gemini_ack.json with status confirm|revise only",
            "Do not reintroduce Python/numpy or fake cargo subcommands",
            "Add only net-new physics not already in affirmations",
        ],
        "repo_anchors": {"knobs": REAL_KNOBS, "sat_run": REAL_RUN},
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--proposal", type=Path, default=INBOX / "gemini_proposal.json")
    ap.add_argument("--out", type=Path, default=OUTBOX / "cursor_response.json")
    args = ap.parse_args()

    proposal = load_json(args.proposal)
    pushbacks = find_pushbacks(proposal)
    resp = build_response(proposal, pushbacks)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(resp, indent=2) + "\n")
    print(json.dumps(resp, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
