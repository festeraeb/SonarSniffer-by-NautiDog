#!/usr/bin/env python3
"""Create a complete skeleton Llama-3 model for wgpu-llm dashboard boot."""
import json, struct, os

# 1. Define Paths
model_dir = os.path.expanduser("~/fake_model")
os.makedirs(model_dir, exist_ok=True)

# 2. Complete Llama-3 Config (Every Mandatory Field)
vocab, hidden, inter, layers = 128, 16, 32, 1
config = {
    "model_type": "llama", "vocab_size": vocab, "hidden_size": hidden,
    "intermediate_size": inter, "num_attention_heads": 2, "num_hidden_layers": layers,
    "num_key_value_heads": 2, "hidden_act": "silu", "max_position_embeddings": 2048,
    "initializer_range": 0.02, "rms_norm_eps": 1e-6, "use_cache": True, "rope_theta": 10000.0
}
with open(os.path.join(model_dir, "config.json"), "w") as f:
    json.dump(config, f)

# 3. Llama-3 Tokenizer with Mandatory 'Special' Flag
tokenizer = {
    "version": "1.0", "added_tokens": [
        {"id": 0, "content": "<|endoftext|>", "special": True, "single_word": False,
          "lstrip": False, "rstrip": False, "normalized": False}
    ],
    "model": {"type": "BPE", "vocab": {"<|endoftext|>": 0}, "merges": []}
}
with open(os.path.join(model_dir, "tokenizer.json"), "w") as f:
    json.dump(tokenizer, f)

# 4. Bit-Perfect Safetensors with Correct Matrix Shapes (Vocab x Hidden)
tensors = {
    "model.embed_tokens.weight": ([vocab, hidden], vocab * hidden * 4),
    "model.layers.0.self_attn.q_proj.weight": ([hidden, hidden], hidden * hidden * 4),
    "model.layers.0.self_attn.k_proj.weight": ([hidden, hidden], hidden * hidden * 4),
    "model.layers.0.self_attn.v_proj.weight": ([hidden, hidden], hidden * hidden * 4),
    "model.layers.0.self_attn.o_proj.weight": ([hidden, hidden], hidden * hidden * 4),
    "model.layers.0.mlp.gate_proj.weight": ([inter, hidden], inter * hidden * 4),
    "model.layers.0.mlp.up_proj.weight": ([inter, hidden], inter * hidden * 4),
    "model.layers.0.mlp.down_proj.weight": ([hidden, inter], hidden * inter * 4),
    "model.layers.0.input_layernorm.weight": ([hidden], hidden * 4),
    "model.layers.0.post_attention_layernorm.weight": ([hidden], hidden * 4),
    "model.norm.weight": ([hidden], hidden * 4),
    "lm_head.weight": ([vocab, hidden], vocab * hidden * 4),
}

header_dict = {"__metadata__": {"format": "pt"}}
offset = 0
for name, (shape, size) in tensors.items():
    header_dict[name] = {"dtype": "F32", "shape": shape, "data_offsets": [offset, offset + size]}
    offset += size

header_json = json.dumps(header_dict).encode("utf-8")
header_len = struct.pack("<Q", len(header_json))

with open(os.path.join(model_dir, "model.safetensors"), "wb") as f:
    f.write(header_len)
    f.write(header_json)
    f.write(b"\x00" * offset)

print(f"Skeleton model built: {model_dir}")
print(f"  config.json: {os.path.getsize(os.path.join(model_dir, 'config.json'))} bytes")
print(f"  tokenizer.json: {os.path.getsize(os.path.join(model_dir, 'tokenizer.json'))} bytes")
print(f"  model.safetensors: {os.path.getsize(os.path.join(model_dir, 'model.safetensors'))} bytes")
print(f"  Total tensor data: {offset} bytes")
print(f"\nRun: HOST=0.0.0.0 PORT=8085 cargo run --bin wgpu-llm -- --model-dir ~/fake_model")
