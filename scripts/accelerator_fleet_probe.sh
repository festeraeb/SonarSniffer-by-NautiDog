#!/usr/bin/env bash
# Probe Coral TPU + Movidius NCS and fleet HTTP endpoints. Emit JSON for Forge/operators.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
HOST="$(hostname -s)"

json_escape() { python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))'; }

accels="[]"
apex_list=""
if ls /dev/apex_* >/dev/null 2>&1; then
  apex_list=$(ls /dev/apex_* 2>/dev/null | tr '\n' ' ')
  accels=$(python3 <<PY
import json
devs=[d.strip() for d in """$apex_list""".split() if d.strip()]
print(json.dumps([{"type":"tpu","name":"Google Coral Edge TPU","device":d,"available":True} for d in devs]))
PY
)
fi

movidius=""
if lsusb 2>/dev/null | grep -q '03e7:'; then
  movidius=$(lsusb | grep '03e7:' | head -1)
  accels=$(python3 <<PY
import json
base=json.loads('''$accels''' if '''$accels'''.strip() else '[]')
base.append({"type":"vpu","name":"""$movidius""".strip(),"device":"usb","available":True})
print(json.dumps(base))
PY
)
fi

pci_tpu=""
if lspci -nn 2>/dev/null | grep -qiE 'coral|1ac1:089a'; then
  pci_tpu=$(lspci -nn | grep -iE 'coral|1ac1' | head -1)
fi

probe_url() {
  local url=$1
  if curl -sf --max-time 4 "${url%/}/health" >/dev/null 2>&1; then echo true; return; fi
  if curl -sf --max-time 4 "${url%/}/v1/models" >/dev/null 2>&1; then echo true; return; fi
  echo false
}

endpoints=$(python3 <<PY
import json
candidates = [
  ("tpu_infer_c2", "http://10.0.0.201:8092"),
  ("tpu_infer_local", "http://127.0.0.1:8092"),
  ("jitter_t440", "http://10.0.0.61:8180"),
  ("jitter_t440_alt", "http://127.0.0.1:8180"),
  ("coral_jitter_ml350e", "http://10.0.0.201:8190"),
  ("jitter_c2", "http://10.0.0.201:8080"),
  ("detection_t440", "http://10.0.0.61:5580"),
  ("detection_c2", "http://127.0.0.1:5580"),
]
import subprocess
out=[]
for name, url in candidates:
    ok = subprocess.run(["curl","-sf","--max-time","4",url.rstrip("/")+"/health"], capture_output=True).returncode==0
    out.append({"name":name,"url":url,"healthy":ok})
print(json.dumps(out))
PY
)

python3 <<PY
import json, os
report = {
  "host": os.environ.get("HOST", "?"),
  "pci_coral": """$pci_tpu""".strip() or None,
  "accelerators": json.loads("""$accels""") if """$accels""".strip() else [],
  "movidius_lsusb": """$movidius""".strip() or None,
  "endpoints": json.loads("""$endpoints"""),
  "forge_url": os.environ.get("FORGE_URL"),
}
print(json.dumps(report, indent=2))
PY
