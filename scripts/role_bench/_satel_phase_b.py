#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

import requests

out = Path(sys.argv[1])
packet = json.loads((out / "accel_scan_packet.json").read_text())

CODERS_ALL = [
    ("P100-Gemma", "http://10.0.0.61:5001"),
    ("P100-Qwen", "http://10.0.0.61:5002"),
    ("RTX-2060", "http://127.0.0.1:5200"),
    ("GTX-1070", "http://127.0.0.1:5202"),
    ("GTX-1070-5203", "http://127.0.0.1:5203"),
]
CODERS_RTX1070 = [
    ("RTX-2060", os.environ.get("RTX_CODER_URL", "http://127.0.0.1:5200")),
    ("GTX-1070", os.environ.get("GTX_CODER_URL", "http://127.0.0.1:5202")),
]
CODERS = CODERS_RTX1070 if os.environ.get("CODERS_ONLY") == "RTX1070" else CODERS_ALL

sys_msg = (
    "You are the CODER for satellite wreck search. Use only the ACCEL_SCAN_PACKET. "
    "Output the required sections. Under 600 words."
)
user_msg = f"""ACCEL_SCAN_PACKET (TPU glint + Movidius jitter — direct fleet HTTP, not Forge):

{json.dumps(packet, indent=2)}

Deliver exactly:
## Wreck candidates (ranked)
## Cue matrix (glint / dark / clear water / jitter / ripple-same-spot)
## False-positive guards
## Code sketch (file paths + process_tile logic)

Great Lakes / Holloway-style survey context."""

for label, url in CODERS:
    print(f"  → {label} @ {url}", flush=True)
    try:
        requests.get(f"{url.rstrip('/')}/v1/models", timeout=8).raise_for_status()
    except Exception as e:
        print(f"     SKIP offline: {e}")
        (out / f"coder_{label}.json").write_text(json.dumps({"error": "offline", "url": url}))
        continue
    body = {
        "model": "default",
        "messages": [
            {"role": "system", "content": sys_msg},
            {"role": "user", "content": user_msg},
        ],
        "temperature": 0.25,
        "max_tokens": int(os.environ.get("PHASE_B_MAX_TOKENS", "8192")),
    }
    try:
        r = requests.post(f"{url.rstrip('/')}/v1/chat/completions", json=body, timeout=600)
        r.raise_for_status()
        d = r.json()
        msg = d["choices"][0]["message"]
        text = (msg.get("content") or "").strip()
        if msg.get("reasoning_content"):
            text = text + "\n\n<!-- reasoning -->\n" + msg["reasoning_content"]
        rec = {"label": label, "url": url, "text": text, "words": len(text.split()), "usage": d.get("usage")}
        (out / f"coder_{label}.md").write_text(text)
        (out / f"coder_{label}.json").write_text(json.dumps(rec, indent=2))
        print(f"     ok {rec['words']} words")
    except Exception as e:
        (out / f"coder_{label}.json").write_text(json.dumps({"error": str(e), "url": url}))
        print(f"     ERR {e}")
