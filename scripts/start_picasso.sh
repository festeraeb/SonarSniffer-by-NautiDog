#!/bin/bash
# Picasso — TinyLlama-1.1B-Chat on cesarops2 P1000 (CUDA, --maingpu 1)
# Always-on intake brain + n8n + validator/draft.
# Deployed to: cesarops@10.0.0.129:~/start_picasso.sh
exec /home/cesarops/benchmark/koboldcpp \
  --model /mnt/storage/models/TinyLlama-1.1B-Chat-v1.0-Q4_K_M.gguf \
  --port 5571 \
  --usecuda \
  --gpulayers 999 \
  --contextsize 2048 \
  --threads 2 \
  --quiet \
  --maingpu 1
