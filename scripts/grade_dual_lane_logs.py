#!/usr/bin/env python3
"""Grade dual-lane run artifacts and llama server logs."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path


def grade_run_dir(d: Path) -> dict:
    g = {"dir": str(d), "jobs": []}
    for job_dir in sorted(d.glob("job_*")):
        j = {"id": job_dir.name, "files": [p.name for p in job_dir.iterdir() if p.is_file()]}
        r1_syn = job_dir / "route1_03_mixtral_synthesis.md"
        r2_pol = job_dir / "route2_03_qwen_polish.md"
        if r1_syn.exists():
            t = r1_syn.read_text()
            j["route"] = "1"
            j["verdict"] = re.search(r"VERDICT:\s*(\w+)", t, re.I)
            j["verdict"] = j["verdict"].group(1) if j["verdict"] else "MISSING"
            j["words"] = len(t.split())
            j["complete"] = "VERDICT" in t.upper()
        elif r2_pol.exists():
            j["route"] = "2"
            j["words_polish"] = len(r2_pol.read_text().split())
            j["has_plan"] = (job_dir / "route2_01_qwen_plan.md").exists()
            j["has_coder"] = (job_dir / "route2_02_1070_coder.md").exists()
            j["complete"] = j["has_plan"] and j["has_coder"] and j["words_polish"] > 100
        else:
            j["route"] = "?"
            j["complete"] = False
            j["note"] = "no terminal artifact"
        g["jobs"].append(j)
    log = d / "run.log"
    if log.exists():
        lines = log.read_text().splitlines()
        g["last_log"] = lines[-5:]
        g["stuck_on"] = next((l for l in reversed(lines) if "step" in l), None)
    return g


def grade_mixtral_log(path: Path) -> dict:
    if not path.exists():
        return {"error": "missing"}
    t = path.read_text(errors="replace")
    return {
        "path": str(path),
        "cuda_rtx": "CUDA0" in t and "RTX 2060" in t.split("CUDA0")[-1][:200] if "CUDA0" in t else False,
        "should_stop_cancels": t.count("should_stop"),
        "timeout_cancels": t.count("adjust the --timeout"),
        "load_dev": re.search(r"hybrid thinker.*dev=(\S+)", t),
    }


def main() -> None:
    repo = Path(__file__).resolve().parents[1]
    bases = list((repo / "var/role_bench").glob("dual_lane*"))
    report = {"runs": [grade_run_dir(b) for b in sorted(bases)], "mixtral_log": grade_mixtral_log(Path("/tmp/llama-mixtral-5200.log"))}
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
