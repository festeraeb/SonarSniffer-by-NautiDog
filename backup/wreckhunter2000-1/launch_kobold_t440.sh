#!/usr/bin/env bash
# Launch KoboldCpp on T440 with Qwen 14B on port 5003

MODEL_DIR="/home/cesarops/models"
MODEL_FILE="qwen2.5-coder-14b-instruct-q4_k_m.gguf"
MODEL_PATH="$MODEL_DIR/$MODEL_FILE"

if [ ! -f "$MODEL_PATH" ]; then
    echo "Model not found: $MODEL_PATH"
    exit 1
fi

cd /home/cesarops
./koboldcpp --model "$MODEL_PATH" --port 5003 --threads 8 --contextsize 32768 --gpulayers 35