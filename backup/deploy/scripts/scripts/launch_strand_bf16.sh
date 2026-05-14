#!/bin/bash
# Launch Fortytwo_Strand-14B BF16 on dual P100s with RAID KV cache
export TMPDIR=/codebase/tmp
mkdir -p /codebase/kv_cache
mkdir -p /codebase/tmp

pkill -f 'Fortytwo_Strand' 2>/dev/null
sleep 2

/home/cesarops/koboldcpp \
    --model /codebase/models/Fortytwo_Strand-Rust-Coder-14B-BF16.gguf \
    --port 5001 \
    --host 0.0.0.0 \
    --gpulayers 64 \
    --usecublas \
    --contextsize 16384 \
    --threads 8 \
    --flashattention \
    --smartcontext \
    --quiet &

echo "Strand BF16 launching on P100s with RAID KV cache..."
echo "Port: 5001, Context: 16384, SmartContext: ON"
echo "TMPDIR: /codebase/tmp"
sleep 30
curl -s http://127.0.0.1:5001/api/v1/model && echo " - READY" || echo " - STILL LOADING"
