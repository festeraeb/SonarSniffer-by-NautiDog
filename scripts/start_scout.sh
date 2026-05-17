#!/bin/bash
# Scout — DeepSeek-R1-7B on cesarops3 P106-100 (CUDA, mining card, no display)
# Thinker / fallback intake.
# Deployed to: cesarops@10.0.0.41:~/start_scout.sh
exec /home/cesarops/koboldcpp \
  --model /home/cesarops/DeepSeek-R1-7B-Q4_K_M.gguf \
  --port 5100 \
  --usecuda \
  --gpulayers 99 \
  --contextsize 4096 \
  --threads 4 \
  --quiet
