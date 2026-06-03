#!/usr/bin/env bash
# After Q8 download: rerun wgpu analysis on T440 (single NUMA, max ctx).
set -euo pipefail

MODEL="${MODEL:-/mnt/t440/models/Qwen3.6-35B-A3B-UD-Q8_K_XL.gguf}"
EXPECTED="${EXPECTED:-38450000000}"
PORT=5212
VAR="/data/codebase/repos/wreckhunter2000-1/scripts/role_bench/var/role_bench"
BENCH_PY="/data/codebase/repos/wreckhunter2000-1/scripts/role_bench/run_mixtral_wgpu_bench.py"
START="/data/codebase/repos/wreckhunter2000-1/scripts/role_bench/start_cpu_server_maxctx.sh"

[[ -f "$MODEL" ]] || { echo "missing $MODEL"; exit 1; }
[[ $(stat -c%s "$MODEL") -ge $EXPECTED ]] || { echo "incomplete download"; exit 1; }

MODEL_MB=39000 THREADS=28 \
  bash "$START" "$MODEL" "$PORT" 28 "$VAR/qwen_q8_t440_server.log" &

for i in $(seq 1 90); do
  curl -sf "http://127.0.0.1:${PORT}/health" >/dev/null && break
  sleep 10
done

python3 "$BENCH_PY" "http://127.0.0.1:${PORT}" 7200 \
  "$VAR/qwen_q8_cpu_wgpu_analysis.md" qwen "Qwen3.6-35B-A3B UD-Q8_K_XL (T440 max ctx)" 4096

echo "compare:"
echo "  Q4: $VAR/qwen_cpu_wgpu_analysis.md"
echo "  Q8: $VAR/qwen_q8_cpu_wgpu_analysis.md"
