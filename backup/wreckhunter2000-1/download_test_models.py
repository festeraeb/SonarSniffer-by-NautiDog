import os
from huggingface_hub import snapshot_download

def main():
    models_dir = os.path.join(os.path.dirname(__file__), "models", "onnx")
    os.makedirs(models_dir, exist_ok=True)
    phi3_dir = os.path.join(models_dir, "phi3-mini-directml")

    print(f"Downloading Phi-3-mini to {phi3_dir} ...")
    # Grab the DirectML optimized 128-block AWQ ONNX version
    snapshot_download(
        repo_id="microsoft/Phi-3-mini-4k-instruct-onnx",
        allow_patterns=[
            "directml/directml-int4-awq-block-128/*",
            "tokenizer.json",
            "tokenizer_config.json"
        ],
        local_dir=phi3_dir,
        local_dir_use_symlinks=False
    )
    print("Done! Ready to benchmark.")

if __name__ == "__main__":
    main()