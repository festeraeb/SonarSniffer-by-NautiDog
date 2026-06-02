#!/usr/bin/env python3
"""
Spec Thinker roundtrip — Gemini structured OperatorSpec → local compile → optional fleet dispatch.

Isolated from cesarops-forge-v2. Set GEMINI_API_KEY for cloud thinker; use --local-only for RTX/1070.

  export GEMINI_API_KEY=...   # https://aistudio.google.com/apikey
  python3 scripts/spec_thinker_roundtrip.py --intent "Fix corrector repeat cap in forge"
  python3 scripts/spec_thinker_roundtrip.py --local-only --intent "..." --dispatch
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from pathlib import Path
from typing import Any

import requests

SCRIPT_DIR = Path(__file__).resolve().parent
REPO = Path(os.environ.get("REPO", SCRIPT_DIR.parent))
SCHEMA_PATH = SCRIPT_DIR / "spec_thinker_schema.json"

sys.path.insert(0, str(SCRIPT_DIR))
from spec_thinker_compile import (  # noqa: E402
    OperatorSpec,
    load_spec_json,
    spec_to_worker_prompt,
    validate_and_compile_spec,
)

def _load_credentials() -> None:
    if os.environ.get("GEMINI_API_KEY"):
        return
    for cred in (
        SCRIPT_DIR / "credentials.gemini.local.sh",
        SCRIPT_DIR / "credentials.sh",
    ):
        if not cred.is_file():
            continue
        try:
            import subprocess

            out = subprocess.run(
                ["bash", "-c", f'source "{cred}" && env'],
                capture_output=True,
                text=True,
                timeout=10,
                check=False,
            )
            for line in out.stdout.splitlines():
                if line.startswith("GEMINI_") and "=" in line:
                    k, _, v = line.partition("=")
                    os.environ.setdefault(k, v)
            if os.environ.get("GEMINI_API_KEY"):
                return
        except OSError:
            pass


_load_credentials()
GEMINI_API_KEY = os.environ.get("GEMINI_API_KEY", "")
GEMINI_MODEL = os.environ.get("GEMINI_MODEL", "gemini-2.5-flash")
GEMINI_BASE = os.environ.get(
    "GEMINI_API_BASE",
    "https://generativelanguage.googleapis.com/v1beta",
)
LOCAL_SPEC_URLS = os.environ.get(
    "SPEC_LOCAL_URLS",
    "http://127.0.0.1:5200,http://127.0.0.1:5203",
)
LOCAL_WORKER_URL = os.environ.get(
    "SPEC_WORKER_URL",
    os.environ.get("CODER_URL", "http://127.0.0.1:5200"),
)


def _strip_unsupported_schema_keys(node: Any) -> Any:
    """Gemini rejects some JSON Schema keywords the Python SDK also warns about."""
    if isinstance(node, dict):
        out = {}
        for k, v in node.items():
            if k in ("additionalProperties", "$schema"):
                continue
            out[k] = _strip_unsupported_schema_keys(v)
        return out
    if isinstance(node, list):
        return [_strip_unsupported_schema_keys(x) for x in node]
    return node


def load_response_schema() -> dict[str, Any]:
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    return _strip_unsupported_schema_keys(schema)


def system_instruction() -> str:
    return (
        "You are the Core Architect Spec Thinker for cesarops-forge-v2. "
        "Translate loose operator intent into a closed-world OperatorSpec for local P100/RTX coders. "
        "Use only real paths under the provided repo_root. "
        "constraints must include keeping T440 P100 LLM ports (:5001/:5002 on 10.0.0.61) free during satellite runs. "
        "needs_user_approval must be true. "
        "Split work into ordered_tasks with finite explicit_steps and testable done_condition per task."
    )


def call_spec_thinker_gemini(
    user_intent: str,
    codebase_summary: str,
    repo_root: str,
    *,
    model: str,
    timeout_s: int,
) -> OperatorSpec:
    if not GEMINI_API_KEY:
        raise RuntimeError(
            "GEMINI_API_KEY not set. Get a free key at https://aistudio.google.com/apikey"
        )

    url = f"{GEMINI_BASE}/models/{model}:generateContent"
    prompt = (
        f"repo_root: {repo_root}\n\n"
        f"Codebase context:\n{codebase_summary}\n\n"
        f"Operator intent:\n{user_intent}"
    )
    payload = {
        "systemInstruction": {"parts": [{"text": system_instruction()}]},
        "contents": [{"role": "user", "parts": [{"text": prompt}]}],
        "generationConfig": {
            "temperature": 0.2,
            "responseMimeType": "application/json",
            "responseJsonSchema": load_response_schema(),
        },
    }
    resp = requests.post(
        url,
        params={"key": GEMINI_API_KEY},
        json=payload,
        timeout=timeout_s,
    )
    if not resp.ok:
        raise RuntimeError(f"Gemini HTTP {resp.status_code}: {resp.text[:800]}")

    data = resp.json()
    candidates = data.get("candidates") or []
    if not candidates:
        raise RuntimeError(f"Gemini returned no candidates: {json.dumps(data)[:500]}")
    parts = (candidates[0].get("content") or {}).get("parts") or []
    texts = [p.get("text", "") for p in parts if p.get("text")]
    raw = "\n".join(texts).strip()
    if not raw:
        raise RuntimeError("Gemini empty text in structured response")
    spec = load_spec_json(raw)
    if not spec.repo_root:
        spec.repo_root = repo_root
    return spec


def call_spec_thinker_local(
    user_intent: str,
    codebase_summary: str,
    repo_root: str,
    *,
    endpoints: list[str],
    timeout_s: int,
) -> OperatorSpec:
    """Fallback: llama-server on RTX :5200 or 1070 :5203 (no schema enforcement)."""
    schema_hint = json.dumps(load_response_schema(), indent=0)[:3500]
    system = (
        system_instruction()
        + "\n\nOutput ONLY one JSON object matching this schema (no markdown):\n"
        + schema_hint
    )
    user = (
        f"repo_root: {repo_root}\n\nCodebase:\n{codebase_summary}\n\nIntent:\n{user_intent}"
    )
    last_err: Exception | None = None
    for ep in endpoints:
        url = f"{ep.rstrip('/')}/v1/chat/completions"
        try:
            t0 = time.time()
            r = requests.post(
                url,
                json={
                    "model": "default",
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": user},
                    ],
                    "temperature": 0.2,
                    "max_tokens": 2048,
                },
                timeout=timeout_s,
            )
            r.raise_for_status()
            text = r.json()["choices"][0]["message"]["content"].strip()
            spec = load_spec_json(text)
            if not spec.repo_root:
                spec.repo_root = repo_root
            print(f"[local] {ep} ok in {time.time() - t0:.1f}s", file=sys.stderr)
            return spec
        except Exception as e:
            last_err = e
            print(f"[local] {ep} failed: {e}", file=sys.stderr)
    raise RuntimeError(f"All local spec endpoints failed: {last_err}")


def default_codebase_summary(repo_root: Path, max_files: int = 40) -> str:
    """Lightweight context for thinker — avoid huge uploads on free tier."""
    lines = [f"repo_root={repo_root}"]
    for name in (
        "cesarops-forge-v2/cluster_config.toml",
        "config/fleet_manifest.json",
        "scripts/cesarops2_unified_layout.sh",
    ):
        p = repo_root / name
        if p.is_file():
            lines.append(f"--- {name} (exists) ---")
    scripts = sorted((repo_root / "scripts").glob("*.sh"))[:max_files]
    if scripts:
        lines.append("scripts/*.sh sample: " + ", ".join(s.name for s in scripts[:20]))
    return "\n".join(lines)


def dispatch_to_local_fleet(
    spec: OperatorSpec,
    worker_url: str,
    *,
    dry_run: bool,
    max_tokens: int,
    timeout_s: int,
) -> None:
    for idx, task in enumerate(spec.ordered_tasks, start=1):
        prompt = spec_to_worker_prompt(task, spec)
        print(f"\n--- Task {idx}/{len(spec.ordered_tasks)}: {task.target_file_path} ---")
        if dry_run:
            print(prompt[:1200])
            if len(prompt) > 1200:
                print("... [truncated]")
            continue
        url = f"{worker_url.rstrip('/')}/v1/chat/completions"
        r = requests.post(
            url,
            json={
                "model": "default",
                "messages": [
                    {
                        "role": "system",
                        "content": "Closed-world coder. Follow steps exactly. Output code and commands only.",
                    },
                    {"role": "user", "content": prompt},
                ],
                "temperature": 0.15,
                "max_tokens": max_tokens,
            },
            timeout=timeout_s,
        )
        r.raise_for_status()
        out = r.json()["choices"][0]["message"]["content"]
        print(out[:2000])
        if len(out) > 2000:
            print("... [truncated]")


def save_artifacts(out_dir: Path, spec: OperatorSpec, raw_source: str) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "operator_spec.json").write_text(
        json.dumps(
            {
                "spec_version": spec.spec_version,
                "title": spec.title,
                "architecture_overview": spec.architecture_overview,
                "constraints": spec.constraints,
                "repo_root": spec.repo_root,
                "needs_user_approval": spec.needs_user_approval,
                "risks": spec.risks,
                "ordered_tasks": [t.__dict__ for t in spec.ordered_tasks],
            },
            indent=2,
        ),
        encoding="utf-8",
    )
    (out_dir / "source.txt").write_text(raw_source, encoding="utf-8")


def main() -> None:
    ap = argparse.ArgumentParser(description="Spec Thinker roundtrip (Gemini + local compile)")
    ap.add_argument("--intent", required=True, help="Operator natural-language request")
    ap.add_argument("--context", default="", help="Extra codebase context text")
    ap.add_argument("--repo", default=str(REPO), help="Repo root for path validation")
    ap.add_argument("--local-only", action="store_true", help="Skip Gemini; use :5200/:5203")
    ap.add_argument("--gemini-model", default=GEMINI_MODEL)
    ap.add_argument("--local-urls", default=LOCAL_SPEC_URLS, help="Comma-separated llama URLs")
    ap.add_argument("--out", default="", help="Write operator_spec.json here")
    ap.add_argument("--dispatch", action="store_true", help="Send tasks to SPEC_WORKER_URL")
    ap.add_argument("--dispatch-dry-run", action="store_true", help="Print worker prompts only")
    ap.add_argument("--worker-url", default=LOCAL_WORKER_URL)
    ap.add_argument("--allow-missing-context", action="store_true")
    ap.add_argument("--git-check", action="store_true", help="Require git branch spec/*")
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--max-tokens", type=int, default=2048)
    args = ap.parse_args()

    repo_root = Path(args.repo).resolve()
    if not repo_root.is_dir():
        print(f"repo not found: {repo_root}", file=sys.stderr)
        sys.exit(1)

    codebase_summary = args.context or default_codebase_summary(repo_root)

    if args.local_only:
        source = "local"
        endpoints = [u.strip() for u in args.local_urls.split(",") if u.strip()]
        spec = call_spec_thinker_local(
            args.intent,
            codebase_summary,
            str(repo_root),
            endpoints=endpoints,
            timeout_s=args.timeout,
        )
    else:
        source = f"gemini:{args.gemini_model}"
        try:
            spec = call_spec_thinker_gemini(
                args.intent,
                codebase_summary,
                str(repo_root),
                model=args.gemini_model,
                timeout_s=args.timeout,
            )
        except Exception as e:
            print(f"Gemini failed ({e}); trying local fallback …", file=sys.stderr)
            endpoints = [u.strip() for u in args.local_urls.split(",") if u.strip()]
            source = "local_fallback"
            spec = call_spec_thinker_local(
                args.intent,
                codebase_summary,
                str(repo_root),
                endpoints=endpoints,
                timeout_s=args.timeout,
            )

    if not spec.repo_root:
        spec.repo_root = str(repo_root)

    print(f"\nSpec Thinker ({source}): {spec.title}")
    print(spec.architecture_overview[:400])
    print(f"Tasks: {len(spec.ordered_tasks)} | approval required: {spec.needs_user_approval}")

    ok, errors, warnings = validate_and_compile_spec(
        spec,
        repo_root,
        require_existing_context=not args.allow_missing_context,
        git_branch_check=args.git_check,
    )
    if warnings:
        print("\nCompiler warnings:")
        for w in warnings:
            print(f"  - {w}")
    if not ok:
        print("\nCompiler errors:")
        for err in errors:
            print(f"  - {err}")
        sys.exit(2)

    print("\nSpec matrix compiled OK — safe to review before fleet dispatch.")

    ts = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    out_dir = Path(args.out or repo_root / "var" / "spec_thinker" / ts)
    save_artifacts(out_dir, spec, source)
    print(f"Wrote {out_dir / 'operator_spec.json'}")

    if args.dispatch or args.dispatch_dry_run:
        dispatch_to_local_fleet(
            spec,
            args.worker_url,
            dry_run=args.dispatch_dry_run and not args.dispatch,
            max_tokens=args.max_tokens,
            timeout_s=args.timeout,
        )

    if spec.needs_user_approval:
        print("\nHuman gate: review operator_spec.json and reply 'yes' before Forge /send.")


if __name__ == "__main__":
    main()
