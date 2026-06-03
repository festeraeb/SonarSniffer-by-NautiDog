#!/usr/bin/env python3
"""
Dual-lane fleet pipeline (Lane A / Lane B per docs/CESAROPS_FORGE_RUNTIME.md).

  Lane A: Mixtral :5200 → Gemma :5001 (T440 P100) → Mixtral polish
  Lane B: Qwen3.6 MoE CPU :5010 → Qwen14 :5002 (T440 P100) → Qwen MoE CPU :5010

Jobs may run in parallel when GPUs are split (Mixtral CUDA0, Qwen14 CUDA1).
Master prompts: scripts/role_bench/forge_lane_master_prompts.json

Jobs file (JSON array): id, route ("1"|"2"), task
  JOBS_FILE=scripts/role_bench/dual_lane_jobs_round1.json
  JOB_INDEX=0          # run only this job (default: all, sequentially)
  MAX_JOBS=1           # cap how many to run from the queue

Env: OUT, MIXTRAL_URL, GEMMA_URL, QWEN_URL, CODER_P100_URL, FORGE_URL
  (legacy: CODER_1070_URL → CODER_P100_URL)
"""
from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

import requests

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
OUT = Path(os.environ.get("OUT", REPO / "var/role_bench/dual_lane_run"))
OUT.mkdir(parents=True, exist_ok=True)

MIXTRAL = os.environ.get("MIXTRAL_URL", "http://127.0.0.1:5200").rstrip("/")
T440 = os.environ.get("T440_LAN", "10.0.0.61")
GEMMA = os.environ.get("GEMMA_URL", f"http://{T440}:5001").rstrip("/")
QWEN = os.environ.get("QWEN_URL", f"http://{T440}:5010").rstrip("/")
CODER_P100 = os.environ.get(
    "CODER_P100_URL",
    os.environ.get("CODER_1070_URL", f"http://{T440}:5002"),
).rstrip("/")
FORGE = os.environ.get("FORGE_URL", "http://10.0.0.61:9100").rstrip("/")

# 0 or unset = no HTTP timeout (accuracy over speed)
_raw_to = os.environ.get("PIPELINE_CHAT_TIMEOUT", "0")
TIMEOUT = None if _raw_to in ("", "0", "none", "None") else int(_raw_to)
JOB_INDEX = os.environ.get("JOB_INDEX")
MAX_JOBS = int(os.environ.get("MAX_JOBS", "999"))
JOBS_FILE = Path(
    os.environ.get("JOBS_FILE", REPO / "scripts/role_bench/dual_lane_jobs_round1.json")
)

_job_out: Path = OUT

# Same task, different prompt discipline (A/B for lane architecture).
REPO_ANCHOR = """
Repo: wreckhunter2000-1 (cesarops-satellite). NO fictional CLI flags.
Real knobs: Knobs.poc_zscore_threshold, poc_max_candidates, poc_min_separation_px, poc_downsample_max_dim, min_score (types.rs).
POC: poc.rs concept_blue_green_clarity + find_peak_clusters; bug: score/wreck_score use raw metric not capped 0-10 like other concepts.
Run: /data/cargo-target/release/sat-run --spec data/missions/straits_local_run.json --root /data/cesarops/satellite_data
Build: cargo build --release -p cesarops-satellite --features gdal
Style reference: docs/FLEET_TOOL_SPECS.md (named files, acceptance lat/lon, real commands).
"""

R1_PLAN = {
    "tight": (
        "You are THINKER (Mixtral). Write an engineering spec for Gemma coder. "
        + REPO_ANCHOR
        + " Output ## Plan (exact files + Knobs fields + function names), ## Acceptance (GPS + metrics), "
        "## Test commands (copy-paste real cargo/sat-run only). Ban placeholder commands. Under 500 words."
    ),
    "loose": (
        "You are THINKER (Mixtral). Output ## Plan and ## Acceptance for Gemma. Under 400 words."
    ),
}
R1_CODE = {
    "tight": (
        "You are Gemma CODER (P100). Implement ONLY the Mixtral plan using REAL repo paths. "
        + REPO_ANCHOR
        + " Output ## Files touched, ## Diff summary (no full file dumps), ## Verification commands. "
        "If plan invents fake knobs/commands, say FAIL and correct to real API. Under 700 words."
    ),
    "loose": (
        "You are Gemma CODER (P100). Implement only the plan. Repo: wreckhunter2000-1. Under 500 words."
    ),
}
R1_SYN = {
    "tight": (
        "You are Mixtral THINKER grading Gemma against the plan AND the real codebase rules in the task. "
        "FAIL if: fake cargo/sat-run, wrong knob names, no mention of score capping in concept_blue_green_clarity, "
        "no preserve GPS acceptance. Output ## Verdict PASS|PARTIAL|FAIL, ## Gaps (bullet), ## Next (actionable). "
        "Be strict; PASS only if implementable without hallucination."
    ),
    "loose": (
        "You are Mixtral THINKER. Grade vs plan. Output ## Verdict PASS|PARTIAL|FAIL, ## Gaps, ## Next."
    ),
}
R2_PLAN = {
    "tight": (
        "You are Qwen lead (CPU :5010). Write implementer spec for P100 Qwen2.5-Coder-14B. "
        + REPO_ANCHOR
        + " ## Spec ## Files ## Tests (real commands only)."
    ),
    "loose": (
        "You are Qwen lead (CPU). Spec for P100 Qwen14 coder. ## Spec ## Files ## Tests."
    ),
}
R2_CODE = {
    "tight": (
        "You are Qwen2.5-Coder on T440 P100. Implement spec with real paths only. " + REPO_ANCHOR + " Under 600 words."
    ),
    "loose": (
        "You are Qwen2.5-Coder on P100. Implement spec only. Under 500 words."
    ),
}
R2_POLISH = {
    "tight": (
        "You are Qwen POLISHER (CPU). Audit P100 coder draft vs spec and repo reality. "
        "## Polished deliverable ## Supervisor notes ## Verdict PASS|PARTIAL|FAIL for implementation readiness."
    ),
    "loose": (
        "You are Qwen POLISHER (P100). Fix draft, keep scope. ## Polished deliverable ## Supervisor notes."
    ),
}

MASTER_PROMPTS_PATH = Path(
    os.environ.get(
        "FORGE_LANE_PROMPTS",
        REPO / "scripts/role_bench/forge_lane_master_prompts.json",
    )
)
NAUTIVECS = os.environ.get("NAUTIVECS_URL", "http://127.0.0.1:5003").rstrip("/")
CTX_TRUNC = int(os.environ.get("PIPELINE_CTX_CHARS", "14000"))


def _load_master_prompts() -> dict | None:
    if not MASTER_PROMPTS_PATH.is_file():
        return None
    data = json.loads(MASTER_PROMPTS_PATH.read_text())
    return {
        "1": {
            "plan": data["lane_a"]["thinker"],
            "code": data["lane_a"]["coder"],
            "syn": data["lane_a"]["polisher"],
        },
        "2": {
            "plan": data["lane_b"]["thinker"],
            "code": data["lane_b"]["coder"],
            "syn": data["lane_b"]["polisher"],
        },
    }


_MASTER = _load_master_prompts()
if _MASTER:
    R1_PLAN, R1_CODE, R1_SYN = _MASTER["1"]["plan"], _MASTER["1"]["code"], _MASTER["1"]["syn"]
    R2_PLAN, R2_CODE, R2_POLISH = _MASTER["2"]["plan"], _MASTER["2"]["code"], _MASTER["2"]["syn"]


def truncate_ctx(text: str, limit: int = CTX_TRUNC) -> str:
    if len(text) <= limit:
        return text
    return text[:limit] + "\n\n[truncated for context limit]\n"


def memory_snippet(task: str, top_k: int = 3) -> str:
    if os.environ.get("FORGE_MEMORY", "1") not in ("1", "true", "yes"):
        return ""
    try:
        r = requests.post(
            f"{NAUTIVECS}/query",
            json={"query": task, "top_k": top_k},
            timeout=8,
        )
        if not r.ok:
            return ""
        block = r.json().get("context_block") or ""
        if block:
            return f"\n\n[nautivecs/OpenMemory context]\n{block}\n"
    except Exception:
        pass
    return ""


def log(msg: str) -> None:
    line = f"[dual-lane] {msg}"
    print(line, flush=True)
    with (OUT / "run.log").open("a") as f:
        f.write(line + "\n")


def jsave(name: str, text: str) -> None:
    (_job_out / name).write_text(text)


def jsave_json(name: str, obj: object) -> None:
    (_job_out / name).write_text(json.dumps(obj, indent=2))


def chat(url: str, system: str, user: str, max_tokens: int | None = None, temperature: float = 0.25) -> tuple[str, float]:
    if max_tokens is None:
        max_tokens = int(os.environ.get("PIPELINE_MAX_TOKENS", "4096"))
    t0 = __import__("time").time()
    req_kw: dict = {}
    if TIMEOUT is not None:
        req_kw["timeout"] = TIMEOUT
    r = requests.post(
        f"{url}/v1/chat/completions",
        json={
            "model": "default",
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": temperature,
            "max_tokens": max_tokens,
        },
        **req_kw,
    )
    r.raise_for_status()
    msg = r.json()["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    if msg.get("reasoning_content"):
        text += "\n\n<!-- reasoning -->\n" + msg["reasoning_content"]
    return text, round(__import__("time").time() - t0, 1)


def normalize_route(route: str) -> str:
    r = str(route).strip().lower()
    if r.startswith("lane_a") or r in ("1", "a", "lane_a_physics_signal"):
        return "1"
    if r.startswith("lane_b") or r in ("2", "b"):
        return "2"
    return r


def load_jobs() -> list[dict]:
    raw = json.loads(JOBS_FILE.read_text())
    if not isinstance(raw, list):
        raise ValueError("jobs file must be a JSON array")
    jobs = []
    for i, row in enumerate(raw):
        if isinstance(row, str):
            jobs.append({"id": f"job_{i:02d}", "route": "1", "task": row})
        else:
            job = dict(row)
            job["id"] = str(job.get("id", f"job_{i:02d}"))
            job["route"] = normalize_route(job.get("route", "1"))
            job["task"] = str(job["task"])
            job["prompt_profile"] = str(job.get("prompt_profile", "loose")).strip().lower()
            jobs.append(job)
    if JOB_INDEX is not None:
        idx = int(JOB_INDEX)
        if idx < 0 or idx >= len(jobs):
            raise IndexError(f"JOB_INDEX={idx} out of range 0..{len(jobs)-1}")
        return [jobs[idx]]
    return jobs[:MAX_JOBS]


def job_step_prompt(job: dict, step_key: str, fallback_table: dict[str, str], profile: str) -> str:
    pp = job.get("pipeline_prompts") or {}
    step = pp.get(step_key)
    if isinstance(step, dict) and step.get("prompt"):
        return str(step["prompt"]) + REPO_ANCHOR
    return _prof(profile, fallback_table)


def log_experiment(job: dict, rec: dict, job_out: Path) -> None:
    meta = job.get("experiment_metadata")
    if not meta:
        return
    key = meta.get("experiment_key", job.get("id", "job"))
    worked = "error" not in rec
    notes = meta.get("success_metrics", "")
    tracker = REPO / "scripts/role_bench/implementor_tracker.py"
    import subprocess

    subprocess.run(
        [
            sys.executable,
            str(tracker),
            "append",
            f"round={job.get('id', 'job')}",
            f"experiment={key}",
            f"outcome={'pipeline_ok' if worked else 'pipeline_fail'}",
            f"worked={'true' if worked else 'false'}",
            f"job_id={job.get('id', '')}",
            f"lane={rec.get('route', '')}",
            f"artifacts={job_out}",
            f"notes={notes}",
        ],
        check=False,
    )


def require_slots(route: str) -> None:
    need = {
        "1": [("mixtral", MIXTRAL), ("gemma", GEMMA)],
        "2": [("qwen_cpu", QWEN), ("coder_p100", CODER_P100)],
    }
    for name, url in need.get(route, []):
        try:
            requests.get(f"{url}/v1/models", timeout=10).raise_for_status()
        except Exception as e:
            raise RuntimeError(f"{name} down at {url}: {e}") from e


def _prof(profile: str, table: dict[str, str]) -> str:
    return table.get(profile, table["loose"])


def route1(task: str, profile: str = "loose", job: dict | None = None) -> dict:
    job = job or {}
    mem = memory_snippet(task)
    jsave("prompt_profile.txt", f"route1 profile={profile} custom_prompts={bool(job.get('pipeline_prompts'))}\n")
    if job.get("pipeline_prompts"):
        jsave_json("pipeline_prompts.json", job["pipeline_prompts"])
    log(f"  R1 profile={profile} step 1/3 Mixtral plan")
    plan, t1 = chat(
        MIXTRAL,
        job_step_prompt(job, "step_1_thinker_mixtral", R1_PLAN, profile),
        f"TASK:\n{task}{mem}",
        temperature=0.35,
    )
    jsave("route1_01_mixtral_plan.md", plan)

    log("  R1 step 2/3 Gemma code")
    code, t2 = chat(
        GEMMA,
        job_step_prompt(job, "step_2_coder_gemma", R1_CODE, profile),
        f"PLAN:\n{plan}\n\nTASK:\n{task}",
    )
    jsave("route1_02_gemma_code.md", code)

    log("  R1 step 3/3 Mixtral synthesize")
    syn, t3 = chat(
        MIXTRAL,
        job_step_prompt(job, "step_3_polisher_mixtral", R1_SYN, profile),
        truncate_ctx(f"TASK:\n{task}{mem}\n\nPLAN:\n{plan}\n\nGEMMA:\n{code}"),
        temperature=0.2,
    )
    jsave("route1_03_mixtral_synthesis.md", syn)
    return {"route": "1", "prompt_profile": profile, "elapsed": {"plan": t1, "code": t2, "syn": t3}}


def route2(task: str, profile: str = "loose") -> dict:
    mem = memory_snippet(task)
    jsave("prompt_profile.txt", f"route2 profile={profile}\n")
    log(f"  R2 profile={profile} step 1/3 Qwen plan (supervisor)")
    plan, t1 = chat(
        QWEN,
        _prof(profile, R2_PLAN),
        f"TASK:\n{task}{mem}",
        temperature=0.3,
    )
    jsave("route2_01_qwen_plan.md", plan)

    log("  R2 step 2/3 P100 Qwen14 Coder")
    impl, t2 = chat(
        CODER_P100,
        _prof(profile, R2_CODE),
        truncate_ctx(f"SPEC:\n{plan}\n\nTASK:\n{task}"),
    )
    jsave("route2_02_p100_coder.md", impl)

    log("  R2 step 3/3 Qwen polish")
    polished, t3 = chat(
        QWEN,
        _prof(profile, R2_POLISH),
        truncate_ctx(f"SPEC:\n{plan}\n\nDRAFT:\n{impl}\n\nTASK:\n{task}{mem}"),
        temperature=0.15,
    )
    jsave("route2_03_qwen_polish.md", polished)
    return {"route": "2", "prompt_profile": profile, "elapsed": {"plan": t1, "impl": t2, "polish": t3}}


def run_job(job: dict) -> dict:
    global _job_out
    jid = re.sub(r"[^\w.-]+", "_", job["id"])[:64]
    route = job["route"]
    task = job["task"]
    profile = job.get("prompt_profile", "loose")
    _job_out = OUT / f"job_{jid}"
    _job_out.mkdir(parents=True, exist_ok=True)
    jsave("task.md", task)
    log(f"JOB {job['id']} route={route} profile={profile} → {_job_out}")

    require_slots(route)
    if route == "2":
        rec = route2(task, profile)
    else:
        rec = route1(task, profile, job)
    rec["job_id"] = job["id"]
    if job.get("experiment_metadata"):
        rec["experiment_metadata"] = job["experiment_metadata"]
    jsave_json("job_result.json", rec)
    log_experiment(job, rec, _job_out)
    return rec


def main() -> int:
    jobs = load_jobs()
    log(f"queue={JOBS_FILE.name} n_jobs={len(jobs)} OUT={OUT}")
    (OUT / "queue.json").write_text(json.dumps(jobs, indent=2))

    summary = []
    for n, job in enumerate(jobs, 1):
        log(f"─── job {n}/{len(jobs)} ───")
        try:
            summary.append(run_job(job))
        except Exception as e:
            log(f"JOB {job['id']} FAILED: {e}")
            summary.append({"job_id": job["id"], "error": str(e)})
            if os.environ.get("STOP_ON_ERROR", "1") == "1":
                break

    (OUT / "summary.json").write_text(json.dumps(summary, indent=2))
    log("finished — see summary.json and job_* dirs")
    return 0 if all("error" not in s for s in summary) else 1


if __name__ == "__main__":
    sys.exit(main())
