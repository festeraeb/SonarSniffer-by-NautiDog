#!/usr/bin/env bash
# Why Qwen3.6 MoE is not loading on cesarops2 (Cake vs llama).
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
MOE_ID="${CAKE_MODEL_35B:-Qwen/Qwen3.6-35B-A3B-Instruct}"
HF_BIN="${HF_BIN:-/data/cesarops/venvs/hf/bin/hf}"
HF_HOME="${HF_HOME:-$HOME/.cache/huggingface}"
GGUF_TURBO="${GGUF_TURBO:-/mnt/t440/models/Qwen3.6-35B-A3B-MXFP4_MOE.gguf}"
GGUF_Q4="${GGUF_Q4:-/mnt/t440/models/Qwen3.6-35B-A3B-Q4_K_M.gguf}"

echo "=== Qwen3.6 MoE diagnose (c2) ==="
echo "Cake needs: HuggingFace safetensors dir for ${MOE_ID}"
echo "llama needs: GGUF (MXFP4 MoE or Q4_K_M) — separate stack"
echo ""

echo "--- HF auth ---"
if [[ -f "$HOME/.cache/huggingface/token" ]]; then
  echo "  token file: yes"
else
  echo "  token file: NO  ← Cake/hf pull get HTTP 401 on gated Qwen3.6"
fi
if [[ -x "$HF_BIN" ]]; then
  "$HF_BIN" auth whoami 2>&1 | sed 's/^/  /' || true
fi
echo ""

echo "--- Hub API (unauthenticated) ---"
code=$(curl -s -o /dev/null -w '%{http_code}' "https://huggingface.co/api/models/${MOE_ID}")
echo "  GET /api/models/${MOE_ID} → HTTP ${code}"
[[ "$code" == "401" ]] && echo "  → Model is gated or requires login. Run: hf auth login"
echo ""

echo "--- Cake cache (safetensors) ---"
cake list 2>/dev/null | grep -E 'Qwen|MODEL' || cake list 2>/dev/null | head -8
hub="${HF_HOME}/hub/models--Qwen--Qwen3.6-35B-A3B-Instruct"
if [[ -d "$hub" ]]; then
  echo "  hub dir: $hub ($(du -sh "$hub" | cut -f1))"
else
  echo "  hub dir: missing (no safetensors cache on this host)"
fi
for p in "$HOME/cesarops-data/models" "/data/cesarops/models" "/mnt/t440/models"; do
  [[ -d "$p/Qwen3.6-35B-A3B-Instruct" ]] && echo "  local dir: $p/Qwen3.6-35B-A3B-Instruct"
done
echo ""

echo "--- GGUF (llama dual script, NOT Cake) ---"
for f in "$GGUF_TURBO" "$GGUF_Q4" "/data/cesarops/models/ZAYA1-8B-Q4_K_M.gguf"; do
  [[ -f "$f" ]] && echo "  OK $(basename "$f") ($(du -h "$f" | cut -f1))" || echo "  missing $f"
done
echo ""

echo "--- Running inference loaders ---"
pgrep -af 'llama-server.*Qwen3.6' && echo "  llama MoE: RUNNING" || echo "  llama MoE: stopped (:5200)"
pgrep -af 'cake.*Qwen3.6' && echo "  cake MoE: RUNNING" || echo "  cake MoE: not running"
pgrep -af 'cake.*72B|Qwen2.5-72B' | head -3 && echo "  cake 72B: loading/running (often masks MoE via CAKE_FALLBACK_72B)"
echo ""

echo "--- Fix paths ---"
echo "  MoE via Cake:"
echo "    hf auth login"
echo "    HF_HOME=$HF_HOME cake pull ${MOE_ID}"
echo "    CAKE_FALLBACK_72B=0 USE_70B=0 bash scripts/cake/start-cake-c2-hybrid.sh start"
echo "  MoE via llama (GGUF, recommended on c2 today):"
echo "    bash scripts/cesarops2-free-workers.sh   # stop cake 72B if disk/RAM contended"
echo "    bash scripts/cesarops2_qwen36_gemma4_dual.sh start"
