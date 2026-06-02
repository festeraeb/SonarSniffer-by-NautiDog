#!/bin/bash
# Run THIS on cesarops3 (P106-100 6GB). Launches llama-server (not Kobold).
set -u

T440_IP="${T440_IP:-10.0.0.61}"
SHARE_MOUNT="/mnt/cesarops-models"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "[1/4] Mount T440 model share via cifs"
sudo mkdir -p "$SHARE_MOUNT"
if mountpoint -q "$SHARE_MOUNT"; then
    echo "  already mounted"
else
    sudo mount -t cifs "//$T440_IP/cesarops-models" "$SHARE_MOUNT" \
        -o "guest,uid=$(id -u),gid=$(id -g),iocharset=utf8,vers=3.0,rsize=130048,wsize=130048" \
    && echo "  mounted ok" || { echo "  cifs mount failed"; exit 1; }
fi

echo "[2/4] Verify model is reachable"
MODEL="$SHARE_MOUNT/DeepSeek-R1-Distill-Qwen-7B-Uncensored.Q4_K_M.gguf"
if [[ ! -f "$MODEL" ]]; then
    echo "  model not found at $MODEL"
    ls "$SHARE_MOUNT" | head -5
    exit 1
fi
echo "  ok: $(ls -lh "$MODEL" | awk '{print $5}')"

echo "[3/4] Stop existing inference on 5570"
pkill -f "koboldcpp.*5570" 2>/dev/null || true
pkill -f "llama-server.*--port 5570" 2>/dev/null || true
sleep 1

echo "[4/4] Launch DeepSeek-R1 7B on GTX 1060 (port 5570, llama-server)"
MODEL="$MODEL" PORT=5570 DEV=CUDA0 CTX=4096 NGL=99 THREADS=4 \
  LOG=/tmp/llama-r1-7b.log \
  bash "$SCRIPT_DIR/launch_llama_remote.sh"

echo "  wait ~30-60s, then: curl http://localhost:5570/v1/models"
