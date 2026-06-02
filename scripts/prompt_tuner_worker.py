#!/usr/bin/env python3
"""Prompt tuner worker for n8n.

Modes:
- initial: tuned first prompt for the user's actual task (verify OR mission).
- failed: rewrite after FAIL or task-mismatch (false PASS on wrong work).

Self-check is task-kind aware (verify vs mission).
"""

from __future__ import annotations

import json
import os
import re
import time
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Dict, List, Tuple


REPO = Path("/codebase/repos/wreckhunter2000-1")
STATE_DIR = REPO / "var" / "prompt_tuner"
STATE_PATH = STATE_DIR / "state.json"
LATEST_PROMPT_PATH = STATE_DIR / "latest_prompt.txt"

MISSION_KEYWORDS = (
    "satellite",
    "sattelite",
    "shipwreck",
    "downloader",
    "lake mi",
    "lake michigan",
    "sentinel",
    "universal_downloader",
    "sat_mission",
    "download",
    "10 day",
    "stack",
    "images",
    "wreckhunter",
)

VERIFY_KEYWORDS = (
    "cargo check",
    "health",
    "/workers",
    "verify",
    "start.sh",
    "5580",
    "8787",
    "cesarops-detection",
)


@dataclass
class TunerState:
    total_runs: int = 0
    success_runs: int = 0
    fail_runs: int = 0
    mismatch_hits: int = 0
    cargo_missing_hits: int = 0
    timeout_hits: int = 0
    wrong_port_hits: int = 0
    pkill_pattern_hits: int = 0
    rewrite_count: int = 0
    self_check_failures: int = 0
    last_updated: str = ""


def load_state() -> TunerState:
    if not STATE_PATH.exists():
        return TunerState()
    try:
        raw = json.loads(STATE_PATH.read_text(encoding="utf-8"))
        return TunerState(**raw)
    except Exception:
        return TunerState()


def save_state(state: TunerState) -> None:
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    state.last_updated = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    STATE_PATH.write_text(json.dumps(asdict(state), indent=2), encoding="utf-8")


def normalize_text(payload: Dict) -> str:
    parts: List[str] = []
    for key in ("report", "latest_report", "summary", "result", "notes", "task"):
        value = payload.get(key)
        if isinstance(value, str):
            parts.append(value)
    if isinstance(payload.get("table_rows"), list):
        parts.extend(str(x) for x in payload["table_rows"])
    return "\n".join(parts).lower()


def classify_task(task: str) -> str:
    """Return verify (detection smoke) or mission (real pipeline work)."""
    explicit = str(task).strip().lower()
    if explicit in ("verify", "mission"):
        return explicit
    t = task.lower()
    if any(k in t for k in MISSION_KEYWORDS):
        return "mission"
    if any(k in t for k in VERIFY_KEYWORDS) and not any(
        k in t for k in ("satellite", "sattelite", "shipwreck", "downloader")
    ):
        return "verify"
    # Long free-form asks are missions, not generic health checks.
    if len(task.split()) > 12:
        return "mission"
    return "verify"


def detect_signals(text: str) -> Dict[str, bool]:
    return {
        "cargo_missing": bool(
            re.search(r"(cargo: command not found|which cargo.*fail|no such file.*cargo)", text)
        ),
        "timeout": bool(re.search(r"(timeout|empty/time|empty\/timeout)", text)),
        "wrong_port": bool(re.search(r"(8787|wrong port|not listening)", text)),
        "pkill_pattern": bool(re.search(r"(pkill failed|pattern too long)", text)),
        "all_pass": bool(re.search(r"\bpass\b", text)) and not bool(re.search(r"\bfail\b", text)),
    }


def detect_task_mismatch(task: str, report: str) -> bool:
    """True when output PASSes generic verify steps but ignores the mission task."""
    if classify_task(task) != "mission":
        return False
    rep = report.lower()
    addressed = any(
        k in rep
        for k in (
            "sat_mission",
            "universal_downloader",
            "pipelines/satellite",
            "download_satellite",
            "lake_michigan",
            "satellite/",
            "shipwreck",
        )
    )
    generic_only = (
        "cesarops-detection" in rep
        and ("5580/health" in rep or "cargo check" in rep)
        and not addressed
    )
    return generic_only


def verify_directives(signals: Dict[str, bool], mode: str) -> List[str]:
    directives = [
        "No preamble. Execute commands first.",
        "No think_harder on turn 1.",
        "Return evidence-backed PASS/FAIL only.",
        'export PATH="/data/cargo-home/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"',
        "Use cesarops-detection port 5580 unless overridden.",
        "If health/workers fail, tail /tmp/cd-start.log.",
        "Use: pkill -x cesarops-detection || true",
    ]
    if mode == "failed":
        if signals["cargo_missing"]:
            directives.append("Prior failure: cargo missing — export PATH first.")
        if signals["wrong_port"] or signals["timeout"]:
            directives.append("Prior failure: port/timeout — confirm DETECTION_PORT in start.sh.")
        if signals["pkill_pattern"]:
            directives.append("Prior failure: use pkill -x cesarops-detection only.")
    directives.append("SELF-CHECK: every PASS row must cite command output.")
    return directives


def mission_directives(mode: str) -> List[str]:
    directives = [
        "ROLE: execute the USER TASK below — do NOT substitute a generic detection health check.",
        "Work under /codebase/repos/wreckhunter2000-1 (user may say wreckhunter2001 — search repo for satellite paths).",
        "Use tools: read_file, run_command, sat_mission, download_satellite_window, write_file.",
        "Prefer deterministic AWS/no-auth path first: universal_downloader.py --sensors aws.",
        "Do NOT block mission on optional credentialed sources (copernicus/usgs/hls).",
        "No think_harder on turn 1 unless blocked after a real attempt.",
        "Small fix-and-continue allowed; log blockers for later.",
        "Return evidence-backed PASS/FAIL per milestone (command output required).",
        "Do NOT return progress/status narrative; return final table only after real command execution.",
        "A row is FAIL if it has no concrete command output (no placeholders/ellipses).",
    ]
    if mode == "failed":
        directives.append(
            "RETRY: previous output wrongly ran cesarops-detection verify only — run satellite/downloader mission now."
        )
    return directives


def build_verify_prompt(payload: Dict, directives: List[str], mode: str) -> str:
    lane = payload.get("lane", "lane-a")
    phase = payload.get("phase", "VERIFY")
    task = payload.get("task", "Verify cesarops-detection startup and endpoints.")
    port = int(payload.get("detection_port", 5580))
    header = "INITIAL_TUNED" if mode == "initial" else "FAILED_REWRITE"
    return f"""ROLE: CODER | PHASE: {phase} | LANE: {lane} | MODE: {header} | KIND: verify

Work ONLY in /codebase/repos/wreckhunter2000-1.
{chr(10).join(f"- {d}" for d in directives)}

TASK:
{task}

Run exactly:
1) export PATH="/data/cargo-home/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
2) which cargo && cargo --version
3) cd /codebase/repos/wreckhunter2000-1 && cargo check -p cesarops-detection
4) cd /codebase/repos/wreckhunter2000-1 && ./cesarops-detection/scripts/start.sh >/tmp/cd-start.log 2>&1 & echo $!
5) sleep 4 && curl -sf http://127.0.0.1:{port}/health
6) curl -sf http://127.0.0.1:{port}/workers
7) pkill -x cesarops-detection || true
8) if step 5 or 6 fails: tail -n 80 /tmp/cd-start.log

Return ONLY:
| Step | Result | PASS/FAIL |
|------|--------|-----------|
| 1 | ... | ... |
| 2 | ... | ... |
| 3 | ... | ... |
| 4 | ... | ... |
| 5 | ... | ... |
| 6 | ... | ... |
| 7 | ... | ... |
| 8 | ... | ... |
"""


def build_mission_prompt(payload: Dict, directives: List[str], mode: str) -> str:
    lane = payload.get("lane", "lane-a")
    task = payload.get("task", "Execute the requested satellite mission.")
    header = "INITIAL_TUNED" if mode == "initial" else "FAILED_REWRITE"
    return f"""ROLE: CODER | PHASE: EXECUTE | LANE: {lane} | MODE: {header} | KIND: mission

{chr(10).join(f"- {d}" for d in directives)}

USER TASK (preserve intent — do not replace with detection verify):
{task}

Execute milestones in order (use tools; paste command output into Result):
1) Discover paths: find satellite/shipwreck/downloader code (pipelines/satellite, universal_downloader.py, sat_mission_orchestrator.py, wreckhunter scripts).
2) Plan run for BOTH areas using a reliability-first path: run AWS optical first (no auth), then optional extras.
3) Execute real AWS downloads for BOTH areas with command-level evidence:
   - python universal_downloader.py --area lake_michigan_south --dates 2025-01-01 2025-01-03 --sensors aws --max-results 2
   - python universal_downloader.py --area lake_michigan_north --dates 2025-01-01 2025-01-03 --sensors aws --max-results 2
4) Optional (do not fail mission if missing creds): dry-run or selective run for podaac/modis/hls; report explicit blockers like missing COPERNICUS_* / USGS_API_KEY.
5) Record artifacts: downloaded file paths, sizes, counts, and exact failing command/error text.
6) Apply small fixes inline if trivial; otherwise mark FAIL with blocker note.
7) Never stop after planning/discovery; execute milestones 3 and 4 before responding.

Return ONLY:
| Milestone | Evidence (paths/logs/commands) | PASS/FAIL |
|-----------|-------------------------------|-----------|
| 1 discover codebase | ... | ... |
| 2 plan run strategy | ... | ... |
| 3 south AWS run | ... | ... |
| 4 north AWS run | ... | ... |
| 5 optional credentialed sources | ... | ... |
| 6 artifacts + counts | ... | ... |
| 7 fixes applied | ... | ... |
"""


def build_prompt(payload: Dict, directives: List[str], mode: str, task_kind: str) -> str:
    if task_kind == "mission":
        return build_mission_prompt(payload, directives, mode)
    return build_verify_prompt(payload, directives, mode)


def self_check_prompt(prompt: str, task_kind: str) -> Tuple[bool, List[str]]:
    checks: Dict[str, bool] = {
        "has_pass_fail_table": "| PASS/FAIL |" in prompt,
        "preserves_task": "USER TASK" in prompt or "TASK:" in prompt,
    }
    if task_kind == "mission":
        checks["mission_not_verify_only"] = (
            "KIND: mission" in prompt
            and "cesarops-detection/scripts/start.sh" not in prompt.split("USER TASK")[0]
        )
        checks["mentions_satellite_path"] = any(
            k in prompt.lower()
            for k in ("sat_mission", "universal_downloader", "pipelines/satellite", "downloader")
        )
    else:
        checks["has_path_export"] = (
            'export PATH="/data/cargo-home/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"'
            in prompt
        )
        checks["has_port_5580"] = "5580/health" in prompt
    failed = [k for k, v in checks.items() if not v]
    return len(failed) == 0, failed


def rewrite_for_self_check(prompt: str, failed_checks: List[str], task_kind: str) -> str:
    if task_kind == "mission":
        if "has_pass_fail_table" in failed_checks and "| PASS/FAIL |" not in prompt:
            prompt += "\n| Milestone | Evidence | PASS/FAIL |\n"
        return prompt
    fixed = prompt
    if "has_path_export" in failed_checks:
        fixed = fixed.replace(
            "Run exactly:\n",
            'Run exactly:\n1) export PATH="/data/cargo-home/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"\n',
        )
    if "has_port_5580" in failed_checks:
        fixed = re.sub(r"http://127\.0\.0\.1:\d+/health", "http://127.0.0.1:5580/health", fixed)
    return fixed


def main() -> None:
    raw = os.sys.stdin.read().strip()
    payload = json.loads(raw) if raw else {}

    state = load_state()
    mode = str(payload.get("mode", "failed")).strip().lower()
    if mode not in ("initial", "failed"):
        mode = "failed"

    task = str(payload.get("task", "")).strip() or "Verify cesarops-detection startup and endpoints."
    task_kind = classify_task(str(payload.get("task_kind", "")) or task)
    text = normalize_text(payload)
    signals = detect_signals(text)
    mismatch = detect_task_mismatch(task, text)

    if task_kind == "mission":
        directives = mission_directives(mode)
    else:
        directives = verify_directives(signals, mode)

    state.total_runs += 1
    if mode == "failed":
        if signals["all_pass"] and not mismatch:
            state.success_runs += 1
        else:
            state.fail_runs += 1
    if mismatch:
        state.mismatch_hits += 1
    state.cargo_missing_hits += int(signals["cargo_missing"])
    state.timeout_hits += int(signals["timeout"])
    state.wrong_port_hits += int(signals["wrong_port"])
    state.pkill_pattern_hits += int(signals["pkill_pattern"])

    tuned_prompt = build_prompt(payload, directives, mode, task_kind)
    passed, failed_checks = self_check_prompt(tuned_prompt, task_kind)
    if not passed:
        tuned_prompt = rewrite_for_self_check(tuned_prompt, failed_checks, task_kind)
        state.rewrite_count += 1
        state.self_check_failures += 1
    save_state(state)

    if payload.get("write_latest_prompt", True):
        STATE_DIR.mkdir(parents=True, exist_ok=True)
        LATEST_PROMPT_PATH.write_text(tuned_prompt, encoding="utf-8")

    result = {
        "ok": True,
        "mode": mode,
        "task_kind": task_kind,
        "task_mismatch": mismatch,
        "state": asdict(state),
        "signals": signals,
        "directives": directives,
        "self_check": {
            "passed": passed,
            "failed_checks": failed_checks,
            "rewritten": not passed,
        },
        "tuned_prompt": tuned_prompt,
        "latest_prompt_path": str(LATEST_PROMPT_PATH),
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
