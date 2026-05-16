#!/bin/bash
# Run THIS on cesarops3 (10.0.0.41, the 1060 6GB box).
# Mounts T440 model share, launches a 7B coder/thinker (DeepSeek-R1-Distill-Qwen-7B Q4_K_M)
# on port 5570. Picked because it fits 6GB w/ headroom and is great at reasoning tasks
# the orchestrator routes to it.
set -u

T440_IP="10.0.0.61"
SHARE_MOUNT="/mnt/cesarops-models"

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

echo "[3/4] Stop any existing koboldcpp on 5570"
pkill -f "koboldcpp.*5570" 2>/dev/null || true
sleep 1

echo "[4/4] Launch DeepSeek-R1 7B on GTX 1060 (port 5570)"
nohup koboldcpp \
    --model "$MODEL" \
    --port 5570 \
    --usecublas 0 \
    --gpulayers 999 \
    --contextsize 4096 \
    --threads 4 \
    --quiet \
    > /tmp/kobold-r1-7b.log 2>&1 &
PID=$!
echo "  launched PID=$PID, log=/tmp/kobold-r1-7b.log"
echo "  wait ~30-60s, then: curl http://localhost:5570/api/extra/version"
