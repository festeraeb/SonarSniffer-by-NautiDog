#!/usr/bin/env python3
"""Append-only learnings log for implementor rounds (what we tried / did it work)."""
from __future__ import annotations

import json
import os
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
LOG = Path(os.environ.get("IMPLEMENTOR_LOG", REPO / "var/role_bench/implementor_learnings.jsonl"))


def append(
    *,
    round_id: str,
    experiment: str,
    outcome: str,
    worked: bool | None = None,
    job_id: str = "",
    lane: str = "",
    code_change: str = "",
    notes: str = "",
    artifacts: str = "",
) -> None:
    LOG.parent.mkdir(parents=True, exist_ok=True)
    row = {
        "ts": datetime.now(timezone.utc).isoformat(),
        "round": round_id,
        "experiment": experiment,
        "outcome": outcome,
        "worked": worked,
        "job_id": job_id,
        "lane": lane,
        "code_change": code_change,
        "notes": notes,
        "artifacts": artifacts,
    }
    with LOG.open("a") as f:
        f.write(json.dumps(row) + "\n")
    print(json.dumps(row, indent=2))


def summary(last_n: int = 20) -> None:
    if not LOG.is_file():
        print("(empty log)")
        return
    lines = LOG.read_text().strip().splitlines()[-last_n:]
    for line in lines:
        print(line)


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} append|summary")
        sys.exit(1)
    if sys.argv[1] == "summary":
        summary()
    elif sys.argv[1] == "append":
        # KEY=val pairs
        kv = {}
        for arg in sys.argv[2:]:
            if "=" in arg:
                k, v = arg.split("=", 1)
                kv[k] = v
        append(
            round_id=kv.get("round", "r0"),
            experiment=kv.get("experiment", ""),
            outcome=kv.get("outcome", ""),
            worked={"true": True, "false": False}.get(kv.get("worked", "").lower()),
            job_id=kv.get("job_id", ""),
            lane=kv.get("lane", ""),
            code_change=kv.get("code_change", ""),
            notes=kv.get("notes", ""),
            artifacts=kv.get("artifacts", ""),
        )
    else:
        sys.exit(2)
