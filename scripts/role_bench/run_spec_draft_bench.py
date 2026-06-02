#!/usr/bin/env python3
"""
Benchmark local spec-draft models on cesarops2 only (RTX :5200 + GTX 1070 :5203).

Does NOT call Forge for inference and does NOT hit T440 P100 ports (:5001/:5002).

Usage:
  bash scripts/role_bench/run_spec_draft_bench.sh
  python3 scripts/role_bench/run_spec_draft_bench.py --endpoints http://127.0.0.1:5200,http://127.0.0.1:5203

Swap models by restarting llama-server on those ports, then re-run to compare scores.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))
from spec_grader import score_operator_spec  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
PROMPTS_PATH = Path(__file__).resolve().parent / "spec_draft_prompts.json"
FIXTURES_PATH = Path(__file__).resolve().parent / "spec_draft_fixtures.json"

# c2 unified layout — never T440 P100s during satellite
DEFAULT_PORTS = (5200, 5203)
DEFAULT_HOST = os.environ.get("SPEC_BENCH_HOST", "127.0.0.1")


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def probe_endpoint(url: str) -> dict[str, Any]:
    info: dict[str, Any] = {"endpoint": url, "online": False, "model": ""}
    try:
        r = requests.get(f"{url.rstrip('/')}/v1/models", timeout=8)
        r.raise_for_status()
        data = r.json().get("data") or []
        info["online"] = True
        if data:
            info["model"] = data[0].get("id") or data[0].get("name") or ""
    except Exception as e:
        info["error"] = str(e)
    return info


def resolve_endpoints(
    explicit: list[str] | None,
    ports: tuple[int, ...],
    host: str,
    discover_forge: str | None,
    allow_p100: bool,
) -> list[dict[str, Any]]:
    if explicit:
        urls = explicit
    elif discover_forge:
        urls = []
        r = requests.get(f"{discover_forge.rstrip('/')}/cluster/gpus", timeout=30)
        r.raise_for_status()
        for g in r.json().get("gpus") or []:
            if not g.get("endpoint_online"):
                continue
            port = int(g.get("port") or 0)
            node = (g.get("node") or "").lower()
            h = g.get("host") or host
            if port in ports and "t440" not in node and "10.0.0.61" not in str(h):
                urls.append(f"http://{h}:{port}")
        urls = sorted(set(urls))
    else:
        urls = [f"http://{host}:{p}" for p in ports]

    out: list[dict[str, Any]] = []
    for url in urls:
        if not allow_p100 and any(x in url for x in (":5001", ":5002", "10.0.0.61")):
            print(f"skip P100 endpoint {url}", file=sys.stderr)
            continue
        meta = probe_endpoint(url)
        if meta["online"]:
            port = int(url.rsplit(":", 1)[-1])
            meta["id"] = f"c2:{port}"
            meta["port"] = port
            out.append(meta)
        else:
            print(f"offline {url}: {meta.get('error', '?')}", file=sys.stderr)
    return out


def chat_completion(
    endpoint: str,
    system: str,
    user: str,
    timeout_s: int,
    max_tokens: int,
) -> tuple[str, float, str | None]:
    url = f"{endpoint.rstrip('/')}/v1/chat/completions"
    t0 = time.time()
    try:
        resp = requests.post(
            url,
            json={
                "model": "default",
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
                "temperature": 0.2,
                "max_tokens": max_tokens,
            },
            timeout=timeout_s,
        )
        resp.raise_for_status()
        data = resp.json()
        text = (
            data.get("choices", [{}])[0]
            .get("message", {})
            .get("content", "")
            .strip()
        )
        return text, round(time.time() - t0, 2), None
    except Exception as e:
        return "", round(time.time() - t0, 2), str(e)


def write_report(run_dir: Path, rows: list[dict[str, Any]]) -> None:
    lines = ["# Spec draft bench", ""]
    by_fixture: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        by_fixture.setdefault(row["fixture_id"], []).append(row)

    for fid, items in sorted(by_fixture.items()):
        lines.append(f"## {fid}")
        lines.append("")
        lines.append("| endpoint | model | score | latency_s | notes |")
        lines.append("|----------|-------|------:|----------:|-------|")
        for it in sorted(items, key=lambda x: -x["score"]):
            notes = "; ".join(it.get("grader_notes") or [])[:120]
            lines.append(
                f"| `{it['endpoint']}` | {it.get('model', '?')[:40]} | "
                f"{it['score']} | {it['latency_s']} | {notes} |"
            )
        lines.append("")

    (run_dir / "report.md").write_text("\n".join(lines), encoding="utf-8")


def main() -> None:
    ap = argparse.ArgumentParser(description="Bench OperatorSpec drafting on RTX/1070 only")
    ap.add_argument(
        "--endpoints",
        default=os.environ.get("SPEC_BENCH_ENDPOINTS", ""),
        help="Comma-separated llama-server URLs (default :5200,:5203 on c2)",
    )
    ap.add_argument(
        "--ports",
        default=os.environ.get("SPEC_BENCH_PORTS", "5200,5203"),
        help="Ports when endpoints not set",
    )
    ap.add_argument("--host", default=DEFAULT_HOST)
    ap.add_argument("--forge-url", default=os.environ.get("FORGE_URL", ""), help="Optional /cluster/gpus discovery")
    ap.add_argument("--allow-p100", action="store_true", help="Allow T440 P100 endpoints (off by default)")
    ap.add_argument("--fixture", default="", help="Run single fixture id")
    ap.add_argument("--timeout", type=int, default=int(os.environ.get("SPEC_BENCH_TIMEOUT", "300")))
    ap.add_argument("--max-tokens", type=int, default=int(os.environ.get("SPEC_BENCH_MAX_TOKENS", "1536")))
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    explicit = [u.strip() for u in args.endpoints.split(",") if u.strip()] or None
    ports = tuple(int(p) for p in args.ports.split(",") if p.strip())
    discover = args.forge_url or None

    endpoints = resolve_endpoints(explicit, ports, args.host, discover, args.allow_p100)
    if not endpoints:
        print(
            "No online endpoints. Start unified layout:\n"
            "  bash scripts/cesarops2_unified_layout.sh start\n"
            "Or: bash scripts/zaya/start_zaya_1070.sh && ensure :5200 is up",
            file=sys.stderr,
        )
        sys.exit(1)

    prompts = load_json(PROMPTS_PATH)
    fixtures_data = load_json(FIXTURES_PATH)
    fixtures = fixtures_data.get("fixtures") or []
    if args.fixture:
        fixtures = [f for f in fixtures if f.get("id") == args.fixture]
        if not fixtures:
            print(f"Unknown fixture: {args.fixture}", file=sys.stderr)
            sys.exit(1)

    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = Path(args.out or REPO / "var" / "role_bench" / "spec_draft" / ts)
    run_dir.mkdir(parents=True, exist_ok=True)

    meta = {
        "started_at": ts,
        "endpoints": endpoints,
        "fixtures": [f["id"] for f in fixtures],
        "ports_policy": list(ports),
        "p100_allowed": args.allow_p100,
    }
    (run_dir / "meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")

    print(f"Run: {run_dir}")
    for ep in endpoints:
        print(f"  {ep['id']} @ {ep['endpoint']} model={ep.get('model', '?')[:60]}")

    rows: list[dict[str, Any]] = []
    system = prompts["system"]
    user_tpl = prompts["user_template"]

    for fix in fixtures:
        fid = fix["id"]
        user = user_tpl.format(prompt=fix["user_prompt"])
        print(f"\n=== fixture {fid} ===")
        for ep in endpoints:
            url = ep["endpoint"]
            print(f"  → {ep['id']} …", flush=True)
            text, lat, err = chat_completion(
                url, system, user, args.timeout, args.max_tokens
            )
            score = score_operator_spec(text) if not err else None
            row = {
                "fixture_id": fid,
                "endpoint": url,
                "port": ep.get("port"),
                "model": ep.get("model"),
                "latency_s": lat,
                "error": err,
                "raw_text": text,
                "score": score.total if score else 0.0,
                "breakdown": score.breakdown if score else {},
                "grader_notes": score.notes if score else [err or "error"],
                "parsed_spec": score.parsed if score else None,
            }
            rows.append(row)
            out_name = f"{fid}_{ep.get('port', 'x')}.json"
            (run_dir / out_name).write_text(json.dumps(row, indent=2), encoding="utf-8")
            status = "ok" if not err else f"ERR: {err}"
            print(f"     {status} score={row['score']} ({lat}s)")

    (run_dir / "results.json").write_text(json.dumps(rows, indent=2), encoding="utf-8")
    write_report(run_dir, rows)

    # Leaderboard per port across fixtures
    by_port: dict[int, list[float]] = {}
    for r in rows:
        if r.get("error"):
            continue
        p = int(r.get("port") or 0)
        by_port.setdefault(p, []).append(float(r["score"]))
    print("\n=== average score by port ===")
    for p, scores in sorted(by_port.items()):
        avg = sum(scores) / len(scores) if scores else 0.0
        print(f"  :{p}  avg={avg:.1f}  n={len(scores)}")

    print(f"\nDone → {run_dir}/report.md")


if __name__ == "__main__":
    main()
