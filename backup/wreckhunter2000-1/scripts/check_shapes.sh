#!/bin/bash
# Quick check of tensor shapes from the running log or by starting fresh
source /home/cesarops/.cargo/env
cd /codebase/repos/wreckhunter2000-1/cesarops-inference

# Use a tiny Rust program to print tensor shapes
cat > /tmp/check_shapes.py << 'EOF'
import struct, sys

path = "/codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf"
f = open(path, "rb")

magic = struct.unpack("<I", f.read(4))[0]
version = struct.unpack("<I", f.read(4))[0]
n_tensors = struct.unpack("<Q", f.read(8))[0]
n_metadata = struct.unpack("<Q", f.read(8))[0]

print(f"GGUF v{version}: {n_tensors} tensors, {n_metadata} metadata")

# Skip metadata
for _ in range(n_metadata):
    key_len = struct.unpack("<Q", f.read(8))[0]
    key = f.read(key_len).decode("utf-8", errors="replace")
    val_type = struct.unpack("<I", f.read(4))[0]
    
    if val_type == 0:  # u32
        f.read(4)
    elif val_type == 1:  # i32
        f.read(4)
    elif val_type == 2:  # f32
        f.read(4)
    elif val_type == 4:  # u16
        f.read(4)
    elif val_type == 5:  # i16
        f.read(4)
    elif val_type == 7:  # bool
        f.read(1)
    elif val_type == 8:  # string
        slen = struct.unpack("<Q", f.read(8))[0]
        f.read(slen)
    elif val_type == 9:  # array
        arr_type = struct.unpack("<I", f.read(4))[0]
        arr_len = struct.unpack("<Q", f.read(8))[0]
        if arr_type == 8:  # string array
            for _ in range(arr_len):
                slen = struct.unpack("<Q", f.read(8))[0]
                f.read(slen)
        elif arr_type == 10:  # u64 array
            f.read(arr_len * 8)
        else:
            f.read(arr_len * 4)
    elif val_type == 10:  # u64
        f.read(8)
    else:
        f.read(8)
    
    if "feed_forward_length" in key or "intermediate" in key:
        print(f"  META: {key}")

# Read tensor info
targets = ["blk.0.ffn_gate.weight", "blk.0.ffn_up.weight", "blk.0.ffn_down.weight", 
           "blk.0.attn_q.weight", "blk.0.attn_k.weight", "token_embd.weight", "output.weight"]
for _ in range(n_tensors):
    name_len = struct.unpack("<Q", f.read(8))[0]
    name = f.read(name_len).decode("utf-8", errors="replace")
    n_dims = struct.unpack("<I", f.read(4))[0]
    shape = [struct.unpack("<Q", f.read(8))[0] for _ in range(n_dims)]
    quant_type = struct.unpack("<I", f.read(4))[0]
    offset = struct.unpack("<Q", f.read(8))[0]
    
    if name in targets:
        print(f"  {name}: shape={shape}, quant={quant_type}")

EOF

python3 /tmp/check_shapes.py
