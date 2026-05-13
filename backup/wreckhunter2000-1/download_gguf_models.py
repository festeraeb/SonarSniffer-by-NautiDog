#!/usr/bin/env python3
"""
Download GGUF models for KoboldCpp.
"""

import os
from huggingface_hub import hf_hub_download

def download_model(repo_id, filename, local_dir):
    os.makedirs(local_dir, exist_ok=True)
    local_path = os.path.join(local_dir, filename)
    if os.path.exists(local_path):
        print(f"Model already exists: {local_path}")
        return local_path
    print(f"Downloading {filename} from {repo_id}...")
    return hf_hub_download(repo_id=repo_id, filename=filename, local_dir=local_dir)

def main():
    models_dir = "/home/cesarops/models"

    # Phi-3 Mini Q4
    download_model(
        "bartowski/Phi-3-mini-4k-instruct-GGUF",
        "Phi-3-mini-4k-instruct-Q4_K_M.gguf",
        models_dir
    )

    # Qwen 14B Q4
    download_model(
        "Qwen/Qwen2.5-Coder-14B-Instruct-GGUF",
        "qwen2.5-coder-14b-instruct-q4_k_m.gguf",
        models_dir
    )

    print("Models downloaded.")

if __name__ == "__main__":
    main()