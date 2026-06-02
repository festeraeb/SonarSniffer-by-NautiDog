#!/usr/bin/env bash
# Thinker (RTX) → three coders → hand grade. No cross-review/polisher.
# Prefer: bash scripts/role_bench/run_fleet_hetero_pipeline.sh
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${OUT:-$REPO/var/role_bench/forge_dispatch_${STAMP}}"
mkdir -p "$OUT"

THINKER="${THINKER_URL:-http://127.0.0.1:5200}"
CODER_A="${CODER_A_URL:-http://10.0.0.61:5001}"
CODER_B="${CODER_B_URL:-http://10.0.0.61:5002}"
CODER_C="${CODER_C_URL:-http://127.0.0.1:5202}"

mkdir -p "$OUT"

log() { echo "[dispatch] $*" | tee -a "$OUT/run.log"; }

log "=== Forge routing (three-coder + RTX thinker) ==="
curl -sf -X POST "${FORGE_URL}/cluster/routing" \
  -H 'Content-Type: application/json' \
  -d "$(python3 -c "
import json
print(json.dumps({
  'thinker_endpoint': '$THINKER',
  'coder_endpoint': '$CODER_A',
  'draft_endpoint': '$CODER_B',
  'corrector_endpoint': '$CODER_C',
  'reviewer_endpoint': '$CODER_B',
  'chat_agent': 'gemma',
}))
")" | tee "$OUT/forge_routing.json" | python3 -m json.tool >/dev/null 2>&1 || true

curl -sf "${FORGE_URL}/cluster/routing" >"$OUT/forge_routing_status.json" 2>/dev/null || true

export OUT THINKER CODER_A CODER_B CODER_C
python3 << 'PY'
import json, os, time
from pathlib import Path
import requests

OUT = Path(os.environ["OUT"])
THINKER = os.environ["THINKER"].rstrip("/")
CODERS = [
    ("P100-Gemma-MoE", os.environ["CODER_A"].rstrip("/")),
    ("P100-Qwen36", os.environ["CODER_B"].rstrip("/")),
    ("1070-Qwen25-Coder7B", os.environ["CODER_C"].rstrip("/")),
]

def models_ok(url):
    try:
        r = requests.get(f"{url}/v1/models", timeout=8)
        r.raise_for_status()
        return r.json()["data"][0]["id"]
    except Exception as e:
        return f"OFFLINE: {e}"

meta = {"thinker": THINKER, "coders": {}}
for label, url in CODERS:
    meta["coders"][label] = {"url": url, "model": models_ok(url)}
(OUT / "endpoints_at_run.json").write_text(json.dumps(meta, indent=2))

thinker_user = """You are the THINKER and dispatcher for a one-shot fleet test.

## Mission A — Forge request (write this as a spec, not code)
We need a NEW watchdog to replace mission_service_watchdog.sh. It must:
- Read the last GPU slot heartbeat snapshot (per gpu_uuid + port + model_path)
- Bring up the LAST KNOWN model on each card (dynamic), not a fixed triple-stack preset
- Unload/stop the port before reload (same port)
Send your spec as a section "## Dynamic watchdog request for Forge"

## Mission B — Dispatch to three coders
Assign a SMALL concrete coding task to EACH worker (different slice, same repo):
- **P100-Gemma-MoE** — UX/script inventory angle
- **P100-Qwen36** — data/schema/reviewer angle  
- **1070-Qwen25-Coder7B** — implementation sketch for cesarops-detection

Repo: wreckhunter2000-1 under /data/codebase/repos or /mnt/t440/codebase/repos.

Deliver exactly:
## Dynamic watchdog request for Forge
## Worker: P100-Gemma-MoE
(task + acceptance criteria)
## Worker: P100-Qwen36
(task + acceptance criteria)
## Worker: 1070-Qwen25-Coder7B
(task + acceptance criteria)

Under 900 words. No full implementations — handoff only."""

thinker_sys = (
    "You are the THINKER in a multi-agent pipeline. Produce clear handoffs. "
    "Do not write full code unless a 5-line sketch is essential."
)

body = {
    "model": "default",
    "messages": [
        {"role": "system", "content": thinker_sys},
        {"role": "user", "content": thinker_user},
    ],
    "temperature": 0.3,
    "max_tokens": 2048,
}
print(f"  → thinker @ {THINKER}", flush=True)
t0 = time.time()
r = requests.post(f"{THINKER}/v1/chat/completions", json=body, timeout=600)
r.raise_for_status()
d = r.json()
msg = d["choices"][0]["message"]
thinker_text = (msg.get("content") or "").strip()
if msg.get("reasoning_content"):
    thinker_text += "\n\n<!-- reasoning -->\n" + msg["reasoning_content"]
(OUT / "thinker_dispatch.md").write_text(thinker_text)
(OUT / "thinker_dispatch.json").write_text(
    json.dumps(
        {
            "url": THINKER,
            "words": len(thinker_text.split()),
            "elapsed_s": round(time.time() - t0, 1),
            "usage": d.get("usage"),
        },
        indent=2,
    )
)
print(f"     thinker ok {len(thinker_text.split())} words", flush=True)

def extract_section(text: str, heading: str) -> str:
    import re
    pat = rf"(?im)^##\s*{re.escape(heading)}\s*$"
    m = re.search(pat, text)
    if not m:
        return ""
    start = m.end()
    m2 = re.search(r"(?im)^##\s+", text[start:])
    end = start + m2.start() if m2 else len(text)
    return text[start:end].strip()

sections = {
    "P100-Gemma-MoE": extract_section(thinker_text, "Worker: P100-Gemma-MoE"),
    "P100-Qwen36": extract_section(thinker_text, "Worker: P100-Qwen36"),
    "1070-Qwen25-Coder7B": extract_section(thinker_text, "Worker: 1070-Qwen25-Coder7B"),
}
watchdog_spec = extract_section(thinker_text, "Dynamic watchdog request for Forge")
(OUT / "watchdog_request_from_thinker.md").write_text(watchdog_spec or "(not found in thinker output)")

coder_sys = (
    "You are a CODER. Implement only your assigned slice from the thinker handoff. "
    "Include file paths and key logic. Under 500 words."
)

for label, url in CODERS:
    handoff = sections.get(label) or thinker_text
    user = f"""THINKER HANDOFF for {label}:

{handoff}

---
Also available: full thinker dispatch is in context if you need cross-references.
Repo: wreckhunter2000-1 / cesarops-detection / cesarops-forge-v2.
Deliver: paths, core logic or sketch, how to verify. What you did NOT do."""
    print(f"  → {label} @ {url}", flush=True)
    try:
        requests.get(f"{url}/v1/models", timeout=8).raise_for_status()
    except Exception as e:
        rec = {"error": str(e), "url": url}
        (OUT / f"coder_{label}.json").write_text(json.dumps(rec, indent=2))
        print(f"     SKIP {e}", flush=True)
        continue
    t0 = time.time()
    r = requests.post(
        f"{url}/v1/chat/completions",
        json={
            "model": "default",
            "messages": [
                {"role": "system", "content": coder_sys},
                {"role": "user", "content": user},
            ],
            "temperature": 0.25,
            "max_tokens": 1536,
        },
        timeout=600,
    )
    r.raise_for_status()
    d = r.json()
    msg = d["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    if msg.get("reasoning_content"):
        text += "\n\n<!-- reasoning -->\n" + msg["reasoning_content"]
    (OUT / f"coder_{label}.md").write_text(text)
    (OUT / f"coder_{label}.json").write_text(
        json.dumps(
            {
                "label": label,
                "url": url,
                "words": len(text.split()),
                "elapsed_s": round(time.time() - t0, 1),
                "usage": d.get("usage"),
            },
            indent=2,
        )
    )
    print(f"     ok {len(text.split())} words", flush=True)

print(f"OUT={OUT}")
PY

log "done → $OUT"
