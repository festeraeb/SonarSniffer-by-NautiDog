#!/usr/bin/env bash
# cesarops2 research lab — CPU detection stubs + dual-GPU llama-server.
#
# Frees vision-worker GPUs: Scout/Validator/Jitter run on CPU (numpy heuristics).
# Uses both consumer GPUs for LLM endpoints Forge expects from cesarops2:
#   :5200  thinker/coder  (RTX 2060 SUPER, CUDA1 / nvidia-smi GPU 1)
#   :5571  draft/intake    (GTX 1070, CUDA2 / nvidia-smi GPU 2)
#   CUDA0 (P106-100) left free unless Scout worker enabled in cluster_config
#
# cesarops2 — HP ProLiant ML350e Gen8, dual NIC eno1=10.0.0.200 eno2=10.0.0.201
# Run on cesarops2 after mounting T440 NFS:
#   bash /mnt/t440/codebase/repos/wreckhunter2000-1/scripts/cesarops2_research_lab.sh start
#   bash .../cesarops2_research_lab.sh status
#   bash .../cesarops2_research_lab.sh stop
#
set -euo pipefail

if [[ -z "${REPO:-}" ]]; then
  for candidate in \
    /mnt/t440/codebase/repos/wreckhunter2000-1 \
    /codebase/repos/wreckhunter2000-1; do
    if [[ -f "$candidate/scripts/cesarops2_research_lab.sh" ]]; then
      REPO="$candidate"
      break
    fi
  done
fi
REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"

if [[ -z "${MODELS:-}" ]]; then
  for candidate in /mnt/t440/models /models /data/cesarops/local_models; do
    if [[ -d "$candidate" ]]; then
      MODELS="$candidate"
      break
    fi
  done
fi
MODELS="${MODELS:-/mnt/t440/models}"
if [[ -z "${LLAMA:-}" ]]; then
  for c in /home/cesarops/bin/llama-server /home/cesarops/src/llama.cpp/build/bin/llama-server; do
    [[ -x "$c" ]] && LLAMA=$c && break
  done
fi
LLAMA="${LLAMA:-llama-server}"
SIM_PY="${REPO}/cesarops-detection/workers/cpu_sim_workers.py"
DET_BIN="${REPO}/target/release/cesarops-detection"
PID_DIR="${PID_DIR:-/tmp/cesarops2-lab}"
VENV="${VENV:-$HOME/.venvs/cesarops-lab}"
mkdir -p "$PID_DIR"

ensure_venv() {
  if [[ ! -x "$VENV/bin/python" ]]; then
    log "Creating venv $VENV"
    python3 -m venv "$VENV"
    "$VENV/bin/pip" install -q fastapi uvicorn pillow numpy
  fi
  PYTHON="$VENV/bin/python"
}

# LLM models sized for 8GB VRAM each (14B Q4 often OOMs on 2060 with ngl=99)
MODEL_CODER="${MODEL_CODER:-$MODELS/DeepSeek-R1-Distill-Qwen-7B-Uncensored.Q4_K_M.gguf}"
# MTP reviewer on 1070 — prefer Flash-MTP quant from NFS models share
MODEL_DRAFT="${MODEL_DRAFT:-$MODELS/Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf}"
if [[ ! -f "$MODEL_DRAFT" ]]; then
  MODEL_DRAFT="${MODEL_DRAFT_FALLBACK:-$MODELS/Phi-3-mini-4k-instruct-Q4_K_M.gguf}"
fi
CTX_CODER="${CTX_CODER:-4096}"
NGL_CODER="${NGL_CODER:-35}"
NGL_DRAFT="${NGL_DRAFT:-99}"
ENABLE_MTP="${ENABLE_MTP:-1}"
MTP_DRAFT_MAX="${MTP_DRAFT_MAX:-2}"

log() { echo "[lab] $*"; }

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  pkill -f "llama-server.*-port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}

start_llm() {
  local model=$1 port=$2 dev=$3 ctx=$4 tag=$5 ngl=$6
  if [[ ! -f "$model" ]]; then
    log "SKIP $tag — model missing: $model"
    return 1
  fi
  if [[ ! -x "$LLAMA" ]]; then
    log "SKIP $tag — llama-server not found: $LLAMA"
    return 1
  fi
  stop_port "$port"
  local logf="$PID_DIR/llama-${port}.log"
  local mtp_args=()
  if [[ "${ENABLE_MTP}" == "1" ]] && [[ "$model" == *MTP* ]]; then
    mtp_args=(--spec-type draft-mtp --spec-draft-n-max "$MTP_DRAFT_MAX")
  fi
  setsid "$LLAMA" \
    -m "$model" \
    --host 0.0.0.0 \
    --port "$port" \
    -dev "$dev" \
    -ngl "$ngl" \
    -c "$ctx" \
    -t 4 \
    "${mtp_args[@]}" \
    </dev/null >>"$logf" 2>&1 &
  disown 2>/dev/null || true
  echo $! >"$PID_DIR/llama-${port}.pid"
  log "LLM $tag PID=$(cat "$PID_DIR/llama-${port}.pid") port=$port dev=$dev log=$logf"
}

start_cpu_workers() {
  VISION_MODE="${VISION_MODE:-cpu}"
  local vision_script="$REPO/scripts/start_vision_workers.sh"
  if [[ -x "$vision_script" ]]; then
    REPO="$REPO" VISION_MODE="$VISION_MODE" VENV="$VENV" PID_DIR="$PID_DIR" \
      VISION_HOST=127.0.0.1 bash "$vision_script" start
    return
  fi
  if [[ ! -f "$SIM_PY" ]]; then
    log "ERROR: $SIM_PY not found (mount T440 NFS?)"
    exit 1
  fi
  ensure_venv
  pkill -f "cpu_sim_workers.py" 2>/dev/null || true
  sleep 1
  for role in scout validator jitter; do
    local logf="$PID_DIR/cpu-${role}.log"
    setsid "$PYTHON" "$SIM_PY" "$role" --host 127.0.0.1 </dev/null >>"$logf" 2>&1 &
    echo $! >"$PID_DIR/cpu-${role}.pid"
    disown 2>/dev/null || true
    log "CPU sim $role PID=$(cat "$PID_DIR/cpu-${role}.pid")"
  done
  sleep 2
}

start_detection() {
  if [[ ! -x "$DET_BIN" ]]; then
    log "Building cesarops-detection..."
    (cd "$REPO" && cargo build --release -p cesarops-detection)
  fi
  pkill -x cesarops-detection 2>/dev/null || pkill -f "/target/release/cesarops-detection" 2>/dev/null || true
  fuser -k 5580/tcp 2>/dev/null || true
  sleep 1
  export SCOUT_URL="http://127.0.0.1:5570"
  export VALIDATOR_URL="http://127.0.0.1:5572"
  export JITTER_URL="http://127.0.0.1:8080"
  export SCOUT_URLS="http://127.0.0.1:5570"
  export VALIDATOR_URLS="http://127.0.0.1:5572"
  export JITTER_URLS="http://127.0.0.1:8080"
  export DETECTION_PORT=5580
  local logf="$PID_DIR/detection-5580.log"
  setsid "$DET_BIN" </dev/null >>"$logf" 2>&1 &
  echo $! >"$PID_DIR/detection.pid"
  disown 2>/dev/null || true
  log "detection :5580 PID=$(cat "$PID_DIR/detection.pid") workers=localhost CPU sim"
}

probe() {
  local url=$1 name=$2
  if curl -sf --max-time 5 "$url" >/dev/null; then
    log "  OK  $name — $url"
  else
    log "  --  $name — $url (not ready yet)"
  fi
}

cmd_start() {
  log "=== cesarops2 research lab start ==="
  log "GPUs: $(nvidia-smi -L 2>/dev/null | tr '\n' ' ')"

  start_cpu_workers
  start_detection

  # nvidia-smi: GPU0=P106, GPU1=2060 SUPER, GPU2=1070
  # llama.cpp CUDA order (by free VRAM / capability): CUDA0=2060, CUDA1=1070, CUDA2=P106
  start_llm "$MODEL_CODER" 5200 CUDA0 "$CTX_CODER" "coder-mtp-2060" "$NGL_CODER"
  start_llm "$MODEL_DRAFT" 5571 CUDA1 4096 "reviewer-mtp-1070" "$NGL_DRAFT"
  log "MTP enabled=${ENABLE_MTP} draft-n-max=${MTP_DRAFT_MAX} (forge pool → :5571 then :5200)"

  log "Waiting 45s for LLM load..."
  sleep 45
  cmd_status
}

cmd_stop() {
  log "=== stopping lab ==="
  pkill -f "cpu_sim_workers.py" 2>/dev/null || true
  pkill -x cesarops-detection 2>/dev/null || true
  stop_port 5200
  stop_port 5571
  stop_port 5580
  rm -f "$PID_DIR"/*.pid 2>/dev/null || true
  log "stopped"
}

cmd_status() {
  log "=== status ==="
  probe "http://127.0.0.1:5570/health" "scout-cpu"
  probe "http://127.0.0.1:5572/health" "validator-cpu"
  probe "http://127.0.0.1:8080/health" "jitter-cpu"
  probe "http://127.0.0.1:5580/health" "detection"
  probe "http://127.0.0.1:5200/v1/models" "llm-coder"
  # 5571 may be cpu sim OR llama — check models endpoint
  if curl -sf --max-time 3 "http://127.0.0.1:5571/v1/models" >/dev/null 2>&1; then
    probe "http://127.0.0.1:5571/v1/models" "llm-draft"
  else
    probe "http://127.0.0.1:5571/health" "cpu-validator-only"
  fi
  nvidia-smi --query-gpu=index,name,utilization.gpu,memory.used --format=csv 2>/dev/null || true
}

cmd_test_detection() {
  log "=== triple-lock smoke (1 tile) ==="
  local b64
  ensure_venv
  b64=$("$PYTHON" -c "
from PIL import Image; import io, base64
im = Image.new('L', (128, 128), color=90)
# bright center blob → cpu sim should flag anomaly
for y in range(40, 88):
    for x in range(40, 88):
        im.putpixel((x, y), 200)
b = io.BytesIO(); im.save(b, 'PNG')
print(base64.b64encode(b.getvalue()).decode())
" 2>/dev/null || echo "aGVsbG8=")
  local job
  job=$(curl -sf -X POST "http://127.0.0.1:5580/scan" \
    -H "Content-Type: application/json" \
    -d "{\"region\":\"lab_smoke\",\"tiles\":[{\"lat\":42.15,\"lon\":-81.25,\"image_b64\":\"$b64\"}]}" \
    | python3 -c "import sys,json; print(json.load(sys.stdin).get('job_id',''))")
  log "job_id=$job"
  sleep 3
  curl -sf "http://127.0.0.1:5580/scan/$job" | python3 -m json.tool | head -40
}

cmd_test_llm() {
  log "=== LLM smoke (coder :5200) ==="
  curl -sf "http://127.0.0.1:5200/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d '{"model":"x","messages":[{"role":"user","content":"Reply with exactly: lab_ok"}],"max_tokens":16,"temperature":0}' \
    | python3 -c "import sys,json; m=json.load(sys.stdin); print(m['choices'][0]['message']['content'][:200])"
}

case "${1:-start}" in
  start)   cmd_start ;;
  stop)    cmd_stop ;;
  status)  cmd_status ;;
  test-detection) cmd_test_detection ;;
  test-llm) cmd_test_llm ;;
  *)
    echo "Usage: $0 {start|stop|status|test-detection|test-llm}"
    exit 1
    ;;
esac
