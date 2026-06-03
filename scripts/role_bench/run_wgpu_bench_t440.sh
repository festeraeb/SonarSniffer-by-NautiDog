#!/usr/bin/env bash
# Run Mixtral + Qwen wgpu benches on T440 (single NUMA, max ctx via --fit).
set -euo pipefail

LLAMA="${LLAMA:-/home/cesarops/llama.cpp/build/bin/llama-server}"
BENCH_PY="/data/codebase/repos/wreckhunter2000-1/scripts/role_bench/run_mixtral_wgpu_bench.py"
VAR="/data/codebase/repos/wreckhunter2000-1/scripts/role_bench/var/role_bench"
mkdir -p "$VAR"

# ~77 GiB available on T440; leave headroom for OS + prompt working set
THREADS="${THREADS:-28}"
QWEN_MODEL_MB="${QWEN_MODEL_MB:-22000}"
MIXTRAL_MODEL_MB="${MIXTRAL_MODEL_MB:-31000}"
PORT_M=5211
PORT_Q=5210

MIXTRAL_MODEL="${MIXTRAL_MODEL:-/mnt/t440/models/Mixtral-8x7B-Instruct-v0.1.Q5_K_M.gguf}"
QWEN_MODEL="${QWEN_MODEL:-/codebase/models/Qwen3.6-35B-A3B-Q4_K_M.gguf}"

start_server() {
  local port=$1 model=$2 log=$3 model_mb=$4
  MODEL_MB="$model_mb" THREADS="$THREADS" \
    bash /data/codebase/repos/wreckhunter2000-1/scripts/role_bench/start_cpu_server_maxctx.sh \
      "$model" "$port" "$THREADS" "$log" &
  echo "started port=$port model_mb=$model_mb log=$log"
}

wait_health() {
  local port=$1
  python3 - <<PY
import time, requests, sys
port = ${port}
for i in range(360):
    try:
        r = requests.get(f"http://127.0.0.1:{port}/health", timeout=5)
        if r.status_code == 200:
            print("ready", port, r.text[:120])
            sys.exit(0)
    except Exception:
        pass
    time.sleep(10)
print("timeout", port)
sys.exit(1)
PY
}

run_bench() {
  local url=$1 out=$2 model_id=$3 title=$4
  python3 "$BENCH_PY" "$url" 3600 "$out" "$model_id" "$title" 4096
}

echo "=== Mixtral ==="
start_server "$PORT_M" "$MIXTRAL_MODEL" "$VAR/mixtral_t440_server.log" "$MIXTRAL_MODEL_MB"
wait_health "$PORT_M"
run_bench "http://127.0.0.1:${PORT_M}" "$VAR/mixtral_cpu_wgpu_analysis.md" mixtral "Mixtral-8x7B mradermacher Q5_K_M (T440 max ctx)"
pkill -f "llama-server.*--port ${PORT_M}" 2>/dev/null || true
sleep 5

echo "=== Qwen ==="
start_server "$PORT_Q" "$QWEN_MODEL" "$VAR/qwen_t440_server.log" "$QWEN_MODEL_MB"
wait_health "$PORT_Q"
run_bench "http://127.0.0.1:${PORT_Q}" "$VAR/qwen_cpu_wgpu_analysis.md" qwen "Qwen3.6-35B-A3B Q4_K_M (T440 max ctx)"
pkill -f "llama-server.*--port ${PORT_Q}" 2>/dev/null || true

echo "Done. Outputs in $VAR"
