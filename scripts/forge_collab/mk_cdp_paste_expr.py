#!/usr/bin/env python3
"""Build /tmp/cdp_paste_expr.json for cursor-ide-browser browser_cdp Runtime.evaluate."""
import json
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8")
expr = f"""(() => {{
  const msg = {json.dumps(text)};
  const ta = [...document.querySelectorAll('textarea')].find(t => (t.placeholder||'').includes('Ask anything'));
  if (!ta) return {{ok:false, err:'no textarea'}};
  ta.focus(); ta.value = msg;
  ta.dispatchEvent(new Event('input', {{bubbles:true}}));
  ta.dispatchEvent(new Event('change', {{bubbles:true}}));
  return {{ok:true, len: msg.length}};
}})()"""
out = Path(sys.argv[2] if len(sys.argv) > 2 else "/tmp/cdp_paste_expr.json")
out.write_text(json.dumps({"expression": expr, "returnByValue": True}), encoding="utf-8")
print(f"wrote {out} ({len(text)} chars payload)")
