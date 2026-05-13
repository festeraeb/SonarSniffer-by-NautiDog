#!/bin/bash
# Launch wgpu-llm dashboard on cesarops2
# Requires: ~/fake_model/ with config.json, tokenizer.json, model.safetensors
# Dashboard available at: http://100.102.158.111:8085/
#
# If the fake_model dir is missing, rebuild it:
#   python3 ~/create_fake_model.py
#   cd ~/fake_model && wget -q https://huggingface.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0/resolve/main/tokenizer.json -O tokenizer.json
#   cd ~/fake_model && wget -q https://huggingface.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0/resolve/main/tokenizer_config.json -O tokenizer_config.json
#
# Also ensure the Samba mount is up:
#   sudo mount -t cifs //100.72.182.77/cesarops-external /mnt/data-external -o username=cesarops,password=cesarops,vers=3.0

cd ~/benchmark/wgpu-llm

HOST=0.0.0.0 PORT=8085 cargo run --bin wgpu-llm -- --model-dir ~/fake_model --prompt "" --max-tokens 0 --temperature 0
