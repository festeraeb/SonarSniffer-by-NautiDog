#!/usr/bin/env python3
"""Split a collab payload into sequential CDP paste+send chunks (same browser tab).

Use with cursor-ide-browser — NEVER browser_navigate on the open Google AI thread.

Agent workflow:
  1. browser_tabs list → note viewId
  2. browser_lock (no navigate)
  3. For each /tmp/cdp_paste_chunk_NN.json: browser_cdp Runtime.evaluate (params from file)
  4. Wait for reply on last chunk only; scrape with CDP innerText / brace match
  5. browser_lock unlock

Usage:
  python3 scripts/forge_collab/mk_cdp_paste_chunks.py var/forge_collab/outbox/GEMINI_FULL_CONTEXT_ROUND7.md
  python3 scripts/forge_collab/mk_cdp_paste_chunks.py payload.md --max-chars 3200
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

DEFAULT_MAX = 3200


def split_text(text: str, max_chars: int) -> list[str]:
    if len(text) <= max_chars:
        return [text]
    parts: list[str] = []
    buf: list[str] = []
    size = 0
    for block in text.split("\n\n"):
        block = block.strip()
        if not block:
            continue
        need = len(block) + (2 if buf else 0)
        if size + need > max_chars and buf:
            parts.append("\n\n".join(buf))
            buf = [block]
            size = len(block)
        else:
            buf.append(block)
            size += need
    if buf:
        parts.append("\n\n".join(buf))
    return parts


def fill_send_expr(msg: str) -> str:
    return f"""(() => {{
  const msg = {json.dumps(msg)};
  const ta = [...document.querySelectorAll('textarea')].find(t => (t.placeholder||'').includes('Ask anything'));
  if (!ta) return {{ok:false, err:'no textarea'}};
  ta.focus(); ta.value = msg;
  ta.dispatchEvent(new Event('input', {{bubbles:true}}));
  ta.dispatchEvent(new Event('change', {{bubbles:true}}));
  const b = [...document.querySelectorAll('button')].find(x => (x.getAttribute('aria-label')||'').toLowerCase().includes('send'));
  if (b) {{ b.click(); return {{ok:true, len: msg.length, sent: true}}; }}
  return {{ok:true, len: msg.length, sent: false}};
}})()"""


def main() -> int:
    max_chars = DEFAULT_MAX
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if "--max-chars" in sys.argv:
        i = sys.argv.index("--max-chars")
        max_chars = int(sys.argv[i + 1])
    if not args:
        print("usage: mk_cdp_paste_chunks.py <payload.md> [--max-chars N]", file=sys.stderr)
        return 1
    text = Path(args[0]).read_text(encoding="utf-8")
    chunks = split_text(text, max_chars)
    n = len(chunks)
    out_dir = Path("/tmp/cdp_paste_chunks")
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest = []
    for i, body in enumerate(chunks, 1):
        header = f"[CESAROPS collab part {i}/{n} — continue in next message if not last]\n\n"
        msg = header + body if n > 1 else body
        path = out_dir / f"chunk_{i:02d}.json"
        path.write_text(
            json.dumps({"expression": fill_send_expr(msg), "returnByValue": True}),
            encoding="utf-8",
        )
        manifest.append({"part": i, "chars": len(msg), "cdp_json": str(path)})
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"chunks={n} max_chars={max_chars} dir={out_dir}")
    print("RULE: browser_lock on existing tab — do NOT browser_navigate")
    for m in manifest:
        print(f"  part {m['part']}: {m['chars']} chars → {m['cdp_json']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
