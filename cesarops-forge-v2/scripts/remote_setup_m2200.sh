#!/bin/bash
# Run THIS on the ThinkPad M2200 (Tailscale 100.110.214.86).
# Mounts the T440 model share via Tailscale, then launches our cesarops-inference
# engine with the small Qwen 1.5B Q6_K model — the regression test rig.
# This is a regression node on purpose: every engine change gets validated here.
set -u

T440_TAILSCALE="100.72.182.77"  # T440 tailscale0 IP
T440_LAN="10.0.0.61"
SHARE_MOUNT="/mnt/cesarops-models"

# Pick whichever the laptop can reach
T440_IP="$T440_LAN"
if ! ping -c1 -W2 "$T440_LAN" >/dev/null 2>&1; then
    T440_IP="$T440_TAILSCALE"
    echo "  using Tailscale IP $T440_IP (off-LAN)"
else
    echo "  using LAN IP $T440_IP"
fi

echo "[1/4] Mount T440 model share"
sudo mkdir -p "$SHARE_MOUNT"
if mountpoint -q "$SHARE_MOUNT"; then
    echo "  already mounted"
else
    sudo mount -t cifs "//$T440_IP/cesarops-models" "$SHARE_MOUNT" \
        -o "guest,uid=$(id -u),gid=$(id -g),iocharset=utf8,vers=3.0,rsize=130048,wsize=130048" \
    && echo "  mounted ok" || { echo "  cifs mount failed — check firewall/445"; exit 1; }
fi

MODEL="$SHARE_MOUNT/qwen2.5-coder-1.5b-instruct-q6_k.gguf"
[[ -f "$MODEL" ]] || { echo "  model missing: $MODEL"; ls "$SHARE_MOUNT" | head -5; exit 1; }

echo "[2/4] Stop existing engines on 5571"
pkill -f "cesarops-inference.*5571" 2>/dev/null || true
pkill -f "koboldcpp.*5571" 2>/dev/null || true
sleep 1

echo "[3/4] Launch cesarops-inference (NATIVE Rust/wgpu) on M2200"
# The M2200 binary should already be built. If not, rebuild on this node.
BIN="/home/cesarops/wreckhunter2000-1/cesarops-inference/target/release/cesarops-inference"
if [[ ! -x "$BIN" ]]; then
    echo "  building cesarops-inference (one-time, ~5min)..."
    (cd /home/cesarops/wreckhunter2000-1/cesarops-inference && cargo build --release) || exit 1
fi
nohup "$BIN" serve --model "$MODEL" --port 5571 --backend wgpu --gpu 0 \
    > /tmp/cesarops-engine-m2200.log 2>&1 &
echo "  launched PID=$!, log=/tmp/cesarops-engine-m2200.log"

echo "[4/4] Smoke test in 30s"
echo "  sleep 30; curl http://localhost:5571/v1/models"
