#!/usr/bin/env python3
"""Merge gemini_ack.json + cursor_response.json → merged decision + learnings log."""
from __future__ import annotations

import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
COLLAB = REPO / "var" / "forge_collab"
MERGED = COLLAB / "merged"


def main() -> int:
    ack_path = COLLAB / "inbox" / "gemini_ack.json"
    cursor_path = COLLAB / "outbox" / "cursor_response.json"
    if not ack_path.is_file():
        print("missing inbox/gemini_ack.json", file=sys.stderr)
        return 1
    if not cursor_path.is_file():
        print("missing outbox/cursor_response.json", file=sys.stderr)
        return 1

    ack = json.loads(ack_path.read_text())
    cursor = json.loads(cursor_path.read_text())
    rid = ack.get("round_id") or cursor.get("round_id") or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")

    decision = {
        "round_id": rid,
        "gemini_status": ack.get("status", "unknown"),
        "cursor_status": cursor.get("status"),
        "hypothesis": cursor.get("accepted_hypothesis") or ack.get("hypothesis"),
        "gemini_additions": ack.get("additions", []),
        "gemini_pushback": ack.get("pushback_on_cursor", []),
        "execute": ack.get("status") in ("confirm", "confirm_with_additions"),
        "forge_action": cursor.get("forge_action"),
    }
    MERGED.mkdir(parents=True, exist_ok=True)
    out = MERGED / f"round_{rid}_decision.json"
    out.write_text(json.dumps(decision, indent=2) + "\n")
    print(f"wrote {out}")

    if decision["execute"]:
        subprocess.run(
            [
                sys.executable,
                str(REPO / "scripts" / "role_bench" / "implementor_tracker.py"),
                "append",
                f"round={rid}",
                "experiment=gemini_cursor_collab",
                f"outcome=merged_execute",
                "worked=unknown",
                f"notes={(decision.get('hypothesis') or '')[:200]}",
                f"artifacts={out}",
            ],
            check=False,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
