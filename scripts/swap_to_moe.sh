#!/bin/bash
# Swap to Qwen3.6-35B MoE on P100s
export TMPDIR=/codebase/tmp
pkill -f koboldcpp 2>/dev/null
sleep 3
echo "Old model killed. Loading MoE..."

/home/cesarops/koboldcpp \
    --model /codebase/models/Qwen3.6-35B-A3B-MXFP4_MOE.gguf \
    --port 5001 \
    --host 0.0.0.0 \
    --gpulayers 64 \
    --usecublas \
    --contextsize 32768 \
    --threads 16 \
    --flashattention \
    --smartcontext \
    --quiet &

echo "MoE loading on P100s..."
sleep 45
curl -s http://127.0.0.1:5001/api/v1/model && echo " - READY" || echo " - STILL LOADING"
