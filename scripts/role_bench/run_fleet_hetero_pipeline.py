#!/usr/bin/env python3
"""
Fleet hetero pipeline: RTX thinker → 3 distinct coder tasks → cross-review → polisher fallback.

  - P100 Gemma (:5001) reviews P100 Qwen output; Qwen (:5002) reviews Gemma output (parallel).
  - First idle P100 reviewer wins review of 1070 output (race).
  - Artifacts that fail review or were skipped go to CPU polisher (:5010).

Env:
  OUT, FORGE_URL, THINKER_URL, GEMMA_URL, QWEN_URL, CODER_1070_URL, POLISHER_URL
"""
from __future__ import annotations

import json
import os
import re
import time
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from pathlib import Path

import requests

OUT = Path(os.environ.get("OUT", "."))
FORGE_URL = os.environ.get("FORGE_URL", "http://127.0.0.1:9100").rstrip("/")
THINKER = os.environ.get("THINKER_URL", "http://127.0.0.1:5200").rstrip("/")
GEMMA = os.environ.get("GEMMA_URL", "http://10.0.0.61:5001").rstrip("/")
QWEN = os.environ.get("QWEN_URL", "http://10.0.0.61:5002").rstrip("/")
CODER_1070 = os.environ.get("CODER_1070_URL", "http://127.0.0.1:5202").rstrip("/")
POLISHER = os.environ.get("POLISHER_URL", "http://10.0.0.61:5010").rstrip("/")

CODERS = [
    ("P100-Gemma-MoE", GEMMA),
    ("P100-Qwen36", QWEN),
    ("1070-Qwen25-Coder7B", CODER_1070),
]

TIMEOUT_CHAT = int(os.environ.get("PIPELINE_CHAT_TIMEOUT", "600"))


def log(msg: str) -> None:
    line = f"[hetero] {msg}"
    print(line, flush=True)
    with (OUT / "run.log").open("a") as f:
        f.write(line + "\n")


def model_id(url: str) -> str:
    try:
        r = requests.get(f"{url}/v1/models", timeout=8)
        r.raise_for_status()
        return r.json()["data"][0]["id"]
    except Exception as e:
        return f"OFFLINE: {e}"


def chat(url: str, system: str, user: str, max_tokens: int = 1536, temperature: float = 0.25) -> tuple[str, float, dict]:
    t0 = time.time()
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
        timeout=TIMEOUT_CHAT,
    )
    r.raise_for_status()
    d = r.json()
    msg = d["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    if msg.get("reasoning_content"):
        text += "\n\n<!-- reasoning -->\n" + msg["reasoning_content"]
    return text, round(time.time() - t0, 1), d.get("usage") or {}


def extract_section(text: str, heading: str) -> str:
    pat = rf"(?im)^##\s*{re.escape(heading)}\s*$"
    m = re.search(pat, text)
    if not m:
        return ""
    start = m.end()
    m2 = re.search(r"(?im)^##\s+", text[start:])
    end = start + m2.start() if m2 else len(text)
    return text[start:end].strip()


def save_json(name: str, obj: object) -> None:
    (OUT / name).write_text(json.dumps(obj, indent=2))


def save_md(name: str, text: str) -> None:
    (OUT / name).write_text(text)


def verdict_passes(text: str) -> bool:
    u = text.upper()
    if "VERDICT: FAIL" in u or "VERDICT:FAIL" in u:
        return False
    return "VERDICT: PASS" in u or "VERDICT:PASS" in u or "VERDICT: PARTIAL" in u or "VERDICT:PARTIAL" in u


def apply_forge_routing() -> None:
    body = {
        "chat_agent": "gemma",
        "thinker_endpoint": THINKER,
        "coder_endpoint": GEMMA,
        "draft_endpoint": QWEN,
        "corrector_endpoint": CODER_1070,
        "reviewer_endpoint": QWEN,
    }
    try:
        r = requests.post(f"{FORGE_URL}/cluster/routing", json=body, timeout=15)
        r.raise_for_status()
        save_json("forge_routing.json", r.json())
    except Exception as e:
        log(f"Forge routing POST skipped: {e}")
    try:
        r = requests.get(f"{FORGE_URL}/cluster/routing", timeout=15)
        if r.ok:
            save_json("forge_routing_status.json", r.json())
    except Exception:
        pass


def run_thinker() -> str:
    user = """You are the THINKER and dispatcher for a one-shot fleet test.

## Mission A — Forge request (spec only, not code)
We need a NEW watchdog to replace mission_service_watchdog.sh:
- Read last GPU slot heartbeat (gpu_uuid + port + model_path + timestamp)
- Unload/stop port, reload LAST KNOWN model on that port (not fixed triple-stack)
Write: ## Dynamic watchdog request for Forge

## Mission B — Three DIFFERENT coder tasks (mandatory)
Assign exactly ONE unique deliverable per worker. Do NOT repeat the same task.
Workers must not overlap primary deliverables.

- **P100-Gemma-MoE** — UX/script inventory ONLY (tables, CLI wrapper spec for watchdog)
- **P100-Qwen36** — schema + validation/reviewer script ONLY (JSON schema, bash validator outline)
- **1070-Qwen25-Coder7B** — code sketch ONLY (cesarops-detection / process_tile or watchdog glue)

Repo: wreckhunter2000-1 under /data/codebase/repos or /mnt/t440/codebase/repos.

Output exactly these headings (handoff text under each, under 120 words each worker):
## Dynamic watchdog request for Forge
## Worker: P100-Gemma-MoE
## Worker: P100-Qwen36
## Worker: 1070-Qwen25-Coder7B

Total under 900 words. No full implementations."""

    sys_prompt = (
        "You are the THINKER. Produce structured handoffs only. "
        "Each worker section MUST describe a different deliverable. "
        "Put all output in the main content field, not reasoning."
    )
    text, elapsed, usage = chat(THINKER, sys_prompt, user, max_tokens=2048, temperature=0.3)
    save_md("thinker_dispatch.md", text)
    save_json("thinker_dispatch.json", {"url": THINKER, "words": len(text.split()), "elapsed_s": elapsed, "usage": usage})
    log(f"thinker {len(text.split())} words in {elapsed}s")
    return text


def run_coders(thinker_text: str) -> dict[str, dict]:
    sections = {
        label: extract_section(thinker_text, f"Worker: {label}")
        for label, _ in CODERS
    }
    save_md(
        "watchdog_request_from_thinker.md",
        extract_section(thinker_text, "Dynamic watchdog request for Forge") or "(not found)",
    )
    save_json("thinker_handoffs.json", sections)

    coder_sys = (
        "You are a CODER. Implement ONLY your assigned slice from the thinker handoff. "
        "Include file paths and verification steps. Under 500 words."
    )
    results: dict[str, dict] = {}

    def one_coder(label: str, url: str) -> tuple[str, dict]:
        handoff = sections.get(label) or ""
        if not handoff.strip():
            return label, {"error": "empty_handoff", "url": url}
        user = f"""THINKER HANDOFF for {label}:

{handoff}

Repo: wreckhunter2000-1 / cesarops-detection / cesarops-forge-v2.
Deliver: paths, core logic or sketch, how to verify. What you did NOT do."""
        try:
            text, elapsed, usage = chat(url, coder_sys, user)
            save_md(f"coder_{label}.md", text)
            save_json(
                f"coder_{label}.json",
                {"label": label, "url": url, "words": len(text.split()), "elapsed_s": elapsed, "usage": usage},
            )
            log(f"coder {label} {len(text.split())}w {elapsed}s")
            return label, {"ok": True, "text": text, "url": url, "elapsed_s": elapsed, "handoff": handoff}
        except Exception as e:
            save_json(f"coder_{label}.json", {"label": label, "url": url, "error": str(e)})
            log(f"coder {label} FAIL {e}")
            return label, {"ok": False, "error": str(e), "url": url, "handoff": handoff}

    with ThreadPoolExecutor(max_workers=3) as ex:
        futs = [ex.submit(one_coder, label, url) for label, url in CODERS]
        for fut in futs:
            label, rec = fut.result()
            results[label] = rec
    return results


def review_prompt(reviewer_label: str, author_label: str, handoff: str, artifact: str) -> tuple[str, str]:
    system = (
        "You are a REVIEWER. Grade only the artifact against the thinker handoff. "
        "End with a line: VERDICT: PASS | PARTIAL | FAIL"
    )
    user = f"""You are {reviewer_label} reviewing output from {author_label}.

THINKER HANDOFF (acceptance criteria):
{handoff or '(none)'}

ARTIFACT TO REVIEW:
{artifact[:12000]}

Check: correct scope, repo paths, meets acceptance criteria, no duplicate of other workers' tasks.
If FAIL, list FIX_AREAS as bullets."""
    return system, user


def run_cross_reviews(coder_results: dict[str, dict]) -> dict[str, dict]:
    gemma_art = coder_results.get("P100-Gemma-MoE", {}).get("text", "")
    qwen_art = coder_results.get("P100-Qwen36", {}).get("text", "")
    gemma_hand = coder_results.get("P100-Gemma-MoE", {}).get("handoff", "")
    qwen_hand = coder_results.get("P100-Qwen36", {}).get("handoff", "")

    reviews: dict[str, dict] = {}

    def do_review(name: str, reviewer_url: str, author: str, handoff: str, artifact: str) -> tuple[str, dict]:
        if not artifact.strip():
            return name, {"skipped": True, "reason": "empty_artifact"}
        sys_p, usr = review_prompt(name, author, handoff, artifact)
        try:
            text, elapsed, usage = chat(reviewer_url, sys_p, usr, max_tokens=1024, temperature=0.15)
            save_md(f"review_{name}.md", text)
            save_json(
                f"review_{name}.json",
                {
                    "reviewer": name,
                    "url": reviewer_url,
                    "author": author,
                    "elapsed_s": elapsed,
                    "pass": verdict_passes(text),
                    "usage": usage,
                },
            )
            log(f"review {name} pass={verdict_passes(text)} {elapsed}s")
            return name, {"ok": True, "text": text, "pass": verdict_passes(text), "elapsed_s": elapsed}
        except Exception as e:
            save_json(f"review_{name}.json", {"reviewer": name, "error": str(e)})
            return name, {"ok": False, "error": str(e)}

    with ThreadPoolExecutor(max_workers=2) as ex:
        f1 = ex.submit(do_review, "gemma-reviews-qwen", GEMMA, "P100-Qwen36", qwen_hand, qwen_art)
        f2 = ex.submit(do_review, "qwen-reviews-gemma", QWEN, "P100-Gemma-MoE", gemma_hand, gemma_art)
        for fut in (f1, f2):
            name, rec = fut.result()
            reviews[name] = rec
    return reviews


def run_1070_review_race(coder_results: dict[str, dict]) -> dict:
    rec1070 = coder_results.get("1070-Qwen25-Coder7B", {})
    artifact = rec1070.get("text", "")
    handoff = rec1070.get("handoff", "")
    if not artifact.strip():
        return {"skipped": True, "reason": "empty_artifact"}

    def review_on(url: str, lane: str) -> tuple[str, dict]:
        sys_p, usr = review_prompt(lane, "1070-Qwen25-Coder7B", handoff, artifact)
        text, elapsed, usage = chat(url, sys_p, usr, max_tokens=1024, temperature=0.15)
        return lane, {
            "url": url,
            "text": text,
            "elapsed_s": elapsed,
            "pass": verdict_passes(text),
            "usage": usage,
        }

    with ThreadPoolExecutor(max_workers=2) as ex:
        futs = {
            ex.submit(review_on, GEMMA, "P100-Gemma-MoE"): "gemma",
            ex.submit(review_on, QWEN, "P100-Qwen36"): "qwen",
        }
        winner_lane = None
        winner_rec = None
        for fut in wait(futs.keys(), return_when=FIRST_COMPLETED).done:
            try:
                _lane, rec = fut.result()
                winner_lane = futs[fut]
                winner_rec = rec
                break
            except Exception:
                pass
        if winner_rec is None:
            for fut in futs:
                if fut.done():
                    continue
                try:
                    _lane, rec = fut.result(timeout=TIMEOUT_CHAT)
                    winner_lane = futs[fut]
                    winner_rec = rec
                    break
                except Exception:
                    pass

    if winner_rec:
        save_md("review_1070-winner.md", winner_rec["text"])
        save_json(
            "review_1070-winner.json",
            {
                "winner_lane": winner_lane,
                "url": winner_rec["url"],
                "elapsed_s": winner_rec["elapsed_s"],
                "pass": winner_rec["pass"],
            },
        )
        log(f"1070 review winner={winner_lane} pass={winner_rec['pass']}")
        return {"winner_lane": winner_lane, **winner_rec}
    return {"error": "both_reviewers_failed"}


def collect_polisher_queue(
    coder_results: dict[str, dict],
    cross_reviews: dict[str, dict],
    review_1070: dict,
) -> list[dict]:
    queue: list[dict] = []
    cross_map = {
        "P100-Gemma-MoE": cross_reviews.get("qwen-reviews-gemma", {}),
        "P100-Qwen36": cross_reviews.get("gemma-reviews-qwen", {}),
        "1070-Qwen25-Coder7B": review_1070,
    }
    for label, _ in CODERS:
        cr = coder_results.get(label, {})
        if not cr.get("ok"):
            queue.append({"label": label, "reason": "coder_failed", "detail": cr.get("error", "")})
            continue
        rev = cross_map.get(label, {})
        if rev.get("skipped"):
            queue.append({"label": label, "reason": "review_skipped", "detail": rev.get("reason", "")})
        elif rev.get("error"):
            queue.append({"label": label, "reason": "review_error", "detail": rev["error"]})
        elif not rev.get("pass"):
            queue.append(
                {
                    "label": label,
                    "reason": "review_fail_or_partial",
                    "artifact": cr.get("text", ""),
                    "review": rev.get("text", ""),
                }
            )
    return queue


def run_polisher(queue: list[dict]) -> dict:
    if not queue:
        log("polisher skipped — nothing in queue")
        return {"skipped": True}
    if "OFFLINE" in model_id(POLISHER):
        log("polisher offline — writing queue only")
        save_json("polisher_queue.json", queue)
        return {"skipped": True, "reason": "polisher_offline"}

    blocks = []
    for item in queue:
        blocks.append(
            f"### {item['label']} ({item['reason']})\n"
            f"{item.get('artifact', item.get('detail', ''))[:4000]}\n"
            f"Review notes:\n{item.get('review', '')[:2000]}"
        )
    user = (
        "You are the POLISHER. Produce minimal fixes for failed or unreviewed slices below. "
        "One section per label. Under 400 words total.\n\n" + "\n\n".join(blocks)
    )
    sys_p = "Polisher: merge, fix paths, satisfy acceptance criteria. No new scope."
    try:
        text, elapsed, usage = chat(POLISHER, sys_p, user, max_tokens=2048, temperature=0.2)
        save_md("polisher_output.md", text)
        save_json("polisher_output.json", {"elapsed_s": elapsed, "usage": usage, "items": len(queue)})
        log(f"polisher {len(text.split())}w {elapsed}s for {len(queue)} items")
        return {"ok": True, "text": text, "elapsed_s": elapsed}
    except Exception as e:
        save_json("polisher_output.json", {"error": str(e)})
        log(f"polisher FAIL {e}")
        return {"ok": False, "error": str(e)}


def write_summary(
    coder_results: dict,
    cross_reviews: dict,
    review_1070: dict,
    polisher: dict,
    queue: list,
) -> None:
    lines = [
        "# Fleet hetero pipeline summary",
        "",
        "## Endpoints",
        f"- Thinker: {THINKER} — {model_id(THINKER)}",
        f"- Gemma: {GEMMA} — {model_id(GEMMA)}",
        f"- Qwen: {QWEN} — {model_id(QWEN)}",
        f"- 1070: {CODER_1070} — {model_id(CODER_1070)}",
        f"- Polisher: {POLISHER} — {model_id(POLISHER)}",
        "",
        "## Cross-review",
        "- Gemma reviews Qwen output",
        "- Qwen reviews Gemma output",
        f"- 1070: first free reviewer wins ({review_1070.get('winner_lane', 'n/a')})",
        "",
        "## Coder timings",
    ]
    for label, rec in coder_results.items():
        if rec.get("ok"):
            lines.append(f"- {label}: {rec.get('elapsed_s')}s")
        else:
            lines.append(f"- {label}: FAILED {rec.get('error', '')}")
    lines.append("")
    lines.append("## Reviews")
    for name, rec in cross_reviews.items():
        lines.append(f"- {name}: pass={rec.get('pass', rec.get('skipped'))}")
    lines.append(f"- 1070 race: {review_1070}")
    lines.append("")
    lines.append(f"## Polisher queue ({len(queue)} items)")
    for item in queue:
        lines.append(f"- {item['label']}: {item['reason']}")
    lines.append(f"\nPolisher: {polisher}")
    save_md("PIPELINE_SUMMARY.md", "\n".join(lines))


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    meta = {
        "thinker": {"url": THINKER, "model": model_id(THINKER)},
        "gemma": {"url": GEMMA, "model": model_id(GEMMA)},
        "qwen": {"url": QWEN, "model": model_id(QWEN)},
        "coder_1070": {"url": CODER_1070, "model": model_id(CODER_1070)},
        "polisher": {"url": POLISHER, "model": model_id(POLISHER)},
    }
    save_json("endpoints_at_run.json", meta)
    apply_forge_routing()

    thinker_text = run_thinker()
    coder_results = run_coders(thinker_text)
    cross_reviews = run_cross_reviews(coder_results)
    review_1070 = run_1070_review_race(coder_results)
    queue = collect_polisher_queue(coder_results, cross_reviews, review_1070)
    save_json("polisher_queue.json", queue)
    polisher = run_polisher(queue)
    write_summary(coder_results, cross_reviews, review_1070, polisher, queue)
    log(f"done → {OUT}")


if __name__ == "__main__":
    main()
