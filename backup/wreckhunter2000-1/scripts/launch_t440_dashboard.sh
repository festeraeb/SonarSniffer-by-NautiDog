#!/bin/bash
# T440 Dashboard — The Command Center
# 
# The T440 runs TWO web interfaces:
#   1. Forge-v2 (SHKT): http://100.72.182.77:9100/ — AI agent with tools
#   2. KoboldCPP UI:    http://100.72.182.77:5001/ — Direct model interaction
#
# The wgpu-llm dashboard runs on cesarops2:
#   3. wgpu-llm:        http://100.102.158.111:8085/ — GPU status dashboard
#
# To start everything on T440:

# Ensure KoboldCPP (35B MoE) is running
if ! curl -s http://127.0.0.1:5001/api/v1/model > /dev/null 2>&1; then
    echo "Starting KoboldCPP (35B MoE on P100s)..."
    export TMPDIR=/codebase/tmp
    nohup /home/cesarops/koboldcpp \
        --model /codebase/models/Qwen3.6-35B-A3B-MXFP4_MOE.gguf \
        --port 5001 --host 0.0.0.0 \
        --gpulayers 64 --usecublas \
        --contextsize 32768 --threads 16 \
        --flashattention --smartcontext --quiet \
        > /codebase/tmp/moe.log 2>&1 &
    echo "Waiting for model load..."
    sleep 45
fi

# Ensure Forge-v2 (SHKT) is running
if ! curl -s http://127.0.0.1:9100/health > /dev/null 2>&1; then
    echo "Starting Forge-v2..."
    cd /codebase/wreckhunter2000-1/cesarops-forge-v2
    RUST_LOG=cesarops_forge_v2=info nohup ./target/release/cesarops-forge-v2 \
        > /codebase/tmp/forge-v2.log 2>&1 &
    sleep 3
fi

# Ensure nautivecs is running
if ! curl -s http://127.0.0.1:5003/health > /dev/null 2>&1; then
    echo "Starting nautivecs..."
    source ~/.cargo/env
    cd /codebase/wreckhunter2000-1
    nohup ./target/release/nautivecs-cli \
        --endpoint http://localhost:5001/v1 \
        --db-path /mnt/data-external/cesarops/nautivecs/store.json \
        serve --port 5003 \
        > /codebase/tmp/nautivecs.log 2>&1 &
    sleep 3
fi

echo ""
echo "=== T440 COMMAND CENTER ==="
echo "  Forge-v2 (AI Agent): http://100.72.182.77:9100/"
echo "  KoboldCPP (Model):   http://100.72.182.77:5001/"
echo "  nautivecs (Search):  http://100.72.182.77:5003/health"
echo ""
echo "  cesarops2 Dashboard: http://100.102.158.111:8085/"
echo ""
echo "All systems nominal."
