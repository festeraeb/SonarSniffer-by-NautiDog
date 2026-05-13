#!/bin/bash
# Move models to /mnt/data-external and free up main disk
set -e
echo cesarops | sudo -S chown -R cesarops:cesarops /mnt/data-external

# Copy models to external drive
mkdir -p /mnt/data-external/models
cp ~/benchmark/models/*.gguf /mnt/data-external/models/ 2>/dev/null || true
cp -r ~/benchmark/models/qwen25-1.5b /mnt/data-external/models/ 2>/dev/null || true
cp -r ~/benchmark/models/tinyllama-safetensors /mnt/data-external/models/ 2>/dev/null || true

# Remove from main disk
rm -rf ~/benchmark/models
rm -rf ~/benchmark/Crane
rm -rf ~/benchmark/mistralrs
rm -rf ~/benchmark/wgpu-llm/target

# Download the 3B coder model to external drive
cd /mnt/data-external/models
if [ ! -f "qwen2.5-coder-3b-instruct-q4_k_m.gguf" ] || [ $(stat -c%s "qwen2.5-coder-3b-instruct-q4_k_m.gguf" 2>/dev/null || echo 0) -lt 1000000000 ]; then
    echo "Downloading Qwen2.5-Coder-3B-Instruct Q4_K_M..."
    curl -L -o qwen2.5-coder-3b-instruct-q4_k_m.gguf \
        "https://huggingface.co/Qwen/Qwen2.5-Coder-3B-Instruct-GGUF/resolve/main/qwen2.5-coder-3b-instruct-q4_k_m.gguf"
fi

# Symlink models back for convenience
ln -sf /mnt/data-external/models ~/benchmark/models

echo "=== Disk status ==="
df -h / | tail -1
df -h /mnt/data-external | tail -1
echo "=== Models ==="
ls -lh /mnt/data-external/models/*.gguf 2>/dev/null
