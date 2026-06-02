#!/usr/bin/env python3
"""
Multi-round role tournament — inference goes directly to llama-server (not Forge).

After each round: rubric scores + judge opinions (why each model did what it did)
before proceeding. Use --auto to skip the interactive gate.

Env:
  FORGE_URL          discovery only (default http://127.0.0.1:9100)
  JUDGE_URL          opinion writer (default http://10.0.0.201:5200)
  ROLE_BENCH_OUT     output dir (default var/role_bench/runs/<timestamp>)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))
from grader import format_candidate_block, score_round  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
PROMPTS_PATH = Path(__file__).resolve().parent / "prompts.json"


def load_prompts() -> dict[str, Any]:
    return json.loads(PROMPTS_PATH.read_text(encoding="utf-8"))


def discover_endpoints(forge_url: str) -> list[dict[str, Any]]:
    r = requests.get(f"{forge_url.rstrip('/')}/cluster/gpus", timeout=30)
    r.raise_for_status()
    out = []
    for g in r.json().get("gpus") or []:
        if not g.get("endpoint_online"):
            continue
        port = g.get("port") or 0
        host = g.get("host") or "127.0.0.1"
        if port <= 0:
            continue
        out.append(
            {
                "id": g.get("identity_key") or f"{g.get('node')}:{port}",
                "endpoint": f"http://{host}:{port}",
                "node": g.get("node"),
                "gpu_name": g.get("name"),
                "gpu_uuid": g.get("gpu_uuid"),
                "model": g.get("loaded_model") or g.get("model") or "",
            }
        )
    return out


def chat_completion(
    endpoint: str,
    system: str,
    user: str,
    timeout_s: int = 600,
    max_tokens: int = 2048,
) -> tuple[str, float]:
    url = f"{endpoint.rstrip('/')}/v1/chat/completions"
    t0 = time.time()
    body = {
        "model": "default",
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": 0.3,
        "max_tokens": max_tokens,
    }
    resp = requests.post(url, json=body, timeout=timeout_s)
    resp.raise_for_status()
    data = resp.json()
    text = (
        data.get("choices", [{}])[0]
        .get("message", {})
        .get("content", "")
        .strip()
    )
    return text, round(time.time() - t0, 2)


def parse_judge_response(raw: str) -> tuple[list[dict[str, Any]], dict[str, Any] | None]:
    opinions: list[dict[str, Any]] = []
    proceed: dict[str, Any] | None = None
    m_proceed = re.search(r"PROCEED:\s*(\{.*\})\s*$", raw, re.S)
    if m_proceed:
        try:
            proceed = json.loads(m_proceed.group(1))
        except json.JSONDecodeError:
            proceed = {"raw_proceed": m_proceed.group(1)}
        raw = raw[: m_proceed.start()].strip()
    arr_match = re.search(r"\[\s*\{.*\}\s*\]", raw, re.S)
    if arr_match:
        try:
            opinions = json.loads(arr_match.group(0))
        except json.JSONDecodeError:
            pass
    return opinions, proceed


def judge_opinions(
    judge_url: str,
    prompts: dict[str, Any],
    round_name: str,
    candidates: list[dict[str, Any]],
    timeout_s: int = 300,
) -> dict[str, Any]:
    tpl = prompts["judge_opinion"]
    rubric_dims = {
        "thinker": "structure, length, actionable_handoff, constraint_awareness",
        "coder": "paths, code, run_instructions, scope_discipline, depth",
        "reviewer": "structure, comparative, actionable, depth",
    }.get(round_name, "see rubric breakdown per candidate")
    user = tpl["user_template"].format(
        round_name=round_name,
        round_title=prompts[round_name]["title"],
        rubric_dims=rubric_dims,
        candidates_block=format_candidate_block(candidates),
    )
    raw, latency = chat_completion(
        judge_url,
        tpl["system"],
        user,
        timeout_s=timeout_s,
        max_tokens=4096,
    )
    opinions, proceed = parse_judge_response(raw)
    return {
        "judge_endpoint": judge_url,
        "latency_s": latency,
        "raw": raw,
        "opinions": opinions,
        "proceed": proceed,
    }


def pick_best_id(candidates: list[dict[str, Any]], judge: dict[str, Any]) -> str:
    if judge.get("proceed") and judge["proceed"].get("recommended_id"):
        return str(judge["proceed"]["recommended_id"])
    if judge.get("opinions"):
        best = max(
            judge["opinions"],
            key=lambda o: int(o.get("score_0_100") or 0),
        )
        return str(best.get("candidate_id") or "")
    return max(candidates, key=lambda c: c["rubric"]["total"])["id"]


def pick_worst_id(candidates: list[dict[str, Any]], best_id: str) -> str:
    rest = [c for c in candidates if c["id"] != best_id]
    if not rest:
        return best_id
    return min(rest, key=lambda c: c["rubric"]["total"])["id"]


def write_markdown_opinions(
    path: Path,
    round_name: str,
    candidates: list[dict[str, Any]],
    judge: dict[str, Any],
) -> None:
    lines = [
        f"# Round `{round_name}` — opinions before next step",
        "",
        f"Judge: `{judge.get('judge_endpoint', '?')}` ({judge.get('latency_s')}s)",
        "",
    ]
    opinion_by_id = {o.get("candidate_id"): o for o in judge.get("opinions") or []}
    for c in sorted(candidates, key=lambda x: -x["rubric"]["total"]):
        o = opinion_by_id.get(c["id"], {})
        lines.extend(
            [
                f"## {c['id']}",
                f"- **Endpoint:** {c['endpoint']}",
                f"- **Model:** {c.get('model', '?')}",
                f"- **Rubric:** {c['rubric']['total']} — {c['rubric'].get('breakdown', {})}",
                f"- **Judge score:** {o.get('score_0_100', '—')}",
                f"- **Verdict:** {o.get('verdict', '—')}",
                "",
                "### Strengths",
                *(f"- {s}" for s in o.get("strengths") or ["_(none parsed)_"]),
                "",
                "### Weaknesses",
                *(f"- {w}" for w in o.get("weaknesses") or ["_(none parsed)_"]),
                "",
                "### Fit for this role",
                (o.get("fit_for_this_role") or "_(judge did not return fit text)_"),
                "",
                "### Why not best",
                (o.get("why_not_best") or "_(this may be the recommended winner)_"),
                "",
                "---",
                "",
            ]
        )
    proc = judge.get("proceed") or {}
    lines.extend(
        [
            "## Proceed recommendation",
            f"- **Recommended:** `{proc.get('recommended_id', '?')}`",
            f"- **Summary:** {proc.get('summary', proc.get('raw_proceed', '—'))}",
            f"- **Ready for next round:** {proc.get('ready_for_next_round', True)}",
            "",
        ]
    )
    path.write_text("\n".join(lines), encoding="utf-8")


def gate_pause(auto: bool, opinion_md: Path) -> None:
    print(f"\n{'=' * 60}")
    print(f"OPINIONS written → {opinion_md}")
    print("Review the file above before continuing.")
    print(f"{'=' * 60}\n")
    if auto:
        print("(--auto) proceeding in 3s…")
        time.sleep(3)
        return
    try:
        input("Press Enter to proceed to the next round (Ctrl+C to abort)… ")
    except KeyboardInterrupt:
        print("\nAborted by user.")
        sys.exit(130)


def run_round(
    round_name: str,
    endpoints: list[dict[str, Any]],
    prompts: dict[str, Any],
    user_prompt: str,
    run_dir: Path,
    judge_url: str,
    auto: bool,
    timeout_s: int,
) -> tuple[list[dict[str, Any]], str, str, str, str]:
    print(f"\n>>> Round: {round_name}")
    spec = prompts[round_name]
    submissions: list[dict[str, Any]] = []

    for ep in endpoints:
        print(f"  → {ep['id']} @ {ep['endpoint']} …", flush=True)
        try:
            text, lat = chat_completion(
                ep["endpoint"],
                spec["system"],
                user_prompt,
                timeout_s=timeout_s,
            )
            err = None
        except Exception as e:
            text, lat, err = "", 0.0, str(e)
        rub = score_round(round_name, text)
        submissions.append(
            {
                **ep,
                "text": text,
                "latency_s": lat,
                "error": err,
                "rubric": {
                    "total": rub.total,
                    "breakdown": rub.breakdown,
                    "notes": rub.notes,
                },
            }
        )
        status = "ok" if not err else f"ERR: {err}"
        print(f"     {status} rubric={rub.total} ({lat}s)")

    (run_dir / f"{round_name}_submissions.json").write_text(
        json.dumps(submissions, indent=2), encoding="utf-8"
    )

    print(f"  → Judge opinions @ {judge_url} …", flush=True)
    judge = judge_opinions(judge_url, prompts, round_name, submissions, timeout_s=timeout_s)
    opinion_md = run_dir / f"{round_name}_opinions.md"
    write_markdown_opinions(opinion_md, round_name, submissions, judge)
    (run_dir / f"{round_name}_opinions.json").write_text(
        json.dumps(judge, indent=2), encoding="utf-8"
    )
    gate_pause(auto, opinion_md)

    best_id = pick_best_id(submissions, judge)
    worst_id = pick_worst_id(submissions, best_id)
    best = next(c for c in submissions if c["id"] == best_id)
    worst = next(c for c in submissions if c["id"] == worst_id)
    print(f"  ✓ Selected best: {best_id} (rubric {best['rubric']['total']})")
    print(f"  ✓ Selected worst for reviewer contrast: {worst_id}")
    return submissions, best_id, worst_id, best["text"], worst["text"]


def main() -> None:
    ap = argparse.ArgumentParser(description="GPU role tournament with per-round opinion gates")
    ap.add_argument("--auto", action="store_true", help="Skip interactive Enter between rounds")
    ap.add_argument("--rounds", default="thinker,coder,reviewer", help="Comma-separated subset")
    ap.add_argument("--forge-url", default=os.environ.get("FORGE_URL", "http://127.0.0.1:9100"))
    ap.add_argument("--judge-url", default=os.environ.get("JUDGE_URL", "http://10.0.0.201:5200"))
    ap.add_argument("--out", default="", help="Output directory")
    ap.add_argument("--timeout", type=int, default=600, help="Per-request timeout seconds")
    args = ap.parse_args()

    prompts = load_prompts()
    endpoints = discover_endpoints(args.forge_url)
    if not endpoints:
        print("No online GPU endpoints from Forge /cluster/gpus", file=sys.stderr)
        sys.exit(1)

    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = Path(args.out or REPO / "var" / "role_bench" / "runs" / ts)
    run_dir.mkdir(parents=True, exist_ok=True)

    meta = {
        "started_at": ts,
        "forge_url": args.forge_url,
        "judge_url": args.judge_url,
        "endpoints": endpoints,
        "rounds": args.rounds.split(","),
    }
    (run_dir / "meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    print(f"Run directory: {run_dir}")
    print(f"Endpoints ({len(endpoints)}):")
    for e in endpoints:
        print(f"  - {e['id']}: {e['model'][:50]} @ {e['endpoint']}")

    rounds = [r.strip() for r in args.rounds.split(",") if r.strip()]
    thinker_outline = ""
    best_coder_text = ""
    worst_coder_text = ""
    best_coder_label = ""
    worst_coder_label = ""

    for round_name in rounds:
        if round_name == "thinker":
            user = prompts["thinker"]["user"]
        elif round_name == "coder":
            if not thinker_outline:
                print("Skipping coder — no thinker outline", file=sys.stderr)
                continue
            user = prompts["coder"]["user_template"].format(thinker_outline=thinker_outline)
        elif round_name == "reviewer":
            if not best_coder_text:
                print("Skipping reviewer — no coder outputs", file=sys.stderr)
                continue
            user = prompts["reviewer"]["user_template"].format(
                best_label=best_coder_label,
                best_body=best_coder_text,
                worst_label=worst_coder_label,
                worst_body=worst_coder_text,
            )
        else:
            print(f"Unknown round: {round_name}", file=sys.stderr)
            continue

        subs, best_id, worst_id, best_text, worst_text = run_round(
            round_name,
            endpoints,
            prompts,
            user,
            run_dir,
            args.judge_url,
            args.auto,
            args.timeout,
        )

        if round_name == "thinker":
            thinker_outline = best_text
        elif round_name == "coder":
            best_coder_label = best_id
            worst_coder_label = worst_id
            best_coder_text = best_text
            worst_coder_text = worst_text

    summary = {
        "run_dir": str(run_dir),
        "completed_rounds": rounds,
        "thinker_winner_chars": len(thinker_outline),
    }
    (run_dir / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(f"\nDone. Artifacts under {run_dir}")


if __name__ == "__main__":
    main()
