#!/usr/bin/env bash
# Launch KoboldCpp on i7 with Phi-3 Mini on port 5002

MODEL_DIR="/home/cesarops/models"
MODEL_FILE="Phi-3-mini-4k-instruct-q4_0.gguf"
MODEL_PATH="$MODEL_DIR/$MODEL_FILE"

if [ ! -f "$MODEL_PATH" ]; then
    echo "Model not found: $MODEL_PATH"
    exit 1
fi

cd /home/cesarops
./koboldcpp --model "$MODEL_PATH" --port 5002 --threads 4 --contextsize 4096 --gpulayers 35