#!/usr/bin/env bash
# Stop Cake HF download on T440 RAM (keep P100s for llama). cesarops2 worker optional.
set -euo pipefail
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
bash "${REPO}/scripts/cake/stop-fleet-cluster.sh" 2>/dev/null || true
pkill -f 'cake master.*Qwen2.5-72B' 2>/dev/null || true
pkill -f 'cake worker.*t440-ram' 2>/dev/null || true
echo "[cake] stopped T440 cake master/worker (use p100 Vulkan + local GGUF instead)"
