#!/bin/bash
# Run THIS on cesarops2 (10.0.0.129, the 1070+P1000 box).
# Mounts the T440 samba share with models, then launches FortyTwo Rust 14B
# on the 1070 (port 5200) with TinyLlama validator already running on P1000.
#
# Usage on cesarops2:
#   curl -s http://10.0.0.61/cesarops-forge-v2/scripts/remote_setup_cesarops2.sh -o /tmp/setup.sh
#   bash /tmp/setup.sh
# OR scp it over and run.
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
MODEL="$SHARE_MOUNT/Fortytwo_Strand-Rust-Coder-14B-v1-Q6_K.gguf"
if [[ ! -f "$MODEL" ]]; then
    echo "  model not found at $MODEL"
    ls "$SHARE_MOUNT" | head -5
    exit 1
fi
echo "  ok: $(ls -lh "$MODEL" | awk '{print $5}')"

echo "[3/4] Stop any existing koboldcpp"
pkill -f "koboldcpp.*5200" 2>/dev/null || true
sleep 1

echo "[4/4] Launch FortyTwo Rust 14B on GTX 1070 (port 5200)"
nohup koboldcpp \
    --model "$MODEL" \
    --port 5200 \
    --usecublas 0 \
    --gpulayers 999 \
    --contextsize 8192 \
    --threads 4 \
    --quiet \
    > /tmp/kobold-fortytwo.log 2>&1 &
PID=$!
echo "  launched PID=$PID, log=/tmp/kobold-fortytwo.log"
echo "  wait ~30-90s for load, then: curl http://localhost:5200/api/extra/version"
