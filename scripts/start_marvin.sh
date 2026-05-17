#!/bin/bash
# Marvin — DeepSeek-Coder-V2-Lite on cesarops2 GTX 1070 (CUDA, --maingpu 0)
# Currently disabled in cluster_config (MLA attention KV cache > 8 GB at usable ctx).
# When re-enabled, this is the launch path.
# Deployed to: cesarops@10.0.0.129:~/start_marvin.sh
exec /home/cesarops/benchmark/koboldcpp \
  --model /mnt/storage/models/DeepSeek-Coder-V2-Lite-Instruct-Q4_K_M.gguf \
  --port 5200 \
  --usecuda \
  --gpulayers 999 \
  --contextsize 4096 \
  --threads 4 \
  --quiet \
  --maingpu 0
