#!/usr/bin/env bash
# Record heartbeat for :5002 as Qwen2.5-Coder-14B so dynamic restore does not reload Qwen3.6 MoE.
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
export REPO
export GPU_SLOT_HEARTBEAT_PATH="${GPU_SLOT_HEARTBEAT_PATH:-/data/cesarops/logs/gpu-slot-heartbeat.json}"
export GPU_SLOT_PINNED_RESTORE_5002="${REPO}/scripts/start_qwen14_coder_p100.sh"
export QWEN14_CODER_PORT=5002

if curl -sf --max-time 3 http://127.0.0.1:5002/v1/models >/dev/null; then
  python3 "$REPO/scripts/gpu_slot_heartbeat.py" tick --no-recover
  echo "[pin-qwen14] snapshot from running :5002"
else
  echo "[pin-qwen14] :5002 down — starting Qwen14 then snapshot"
  bash "$REPO/scripts/start_qwen14_coder_p100.sh"
  python3 "$REPO/scripts/gpu_slot_heartbeat.py" tick --no-recover
fi

python3 - <<'PY'
import json, os
from pathlib import Path
p = Path(os.environ["GPU_SLOT_HEARTBEAT_PATH"])
store = json.loads(p.read_text()) if p.is_file() else {"slots": {}}
key = "127.0.0.1:5002"
slot = store.setdefault("slots", {}).get(key) or {}
mp = (slot.get("model_path") or "").lower()
if "qwen3.6" in mp or "35b-a3b" in mp or "qwen3" in mp and "coder-14" not in mp:
    print("[pin-qwen14] WARN: heartbeat still shows non-14B model:", slot.get("model_path"))
else:
    print("[pin-qwen14] OK model_path:", slot.get("model_path", "(empty)"))
PY

echo "[pin-qwen14] set GPU_SLOT_PINNED_RESTORE_5002 in env for heartbeat.py (see docs/GPU_SLOT_WATCHDOG.md)"
