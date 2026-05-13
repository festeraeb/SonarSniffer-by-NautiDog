#!/usr/bin/env python3
import json
path = "/home/cesarops/benchmark/models/tinyllama-safetensors/config.json"
with open(path) as f:
    c = json.load(f)
c["sliding_window"] = c.get("sliding_window", 2048)
with open(path, "w") as f:
    json.dump(c, f, indent=2)
print(f"Fixed - sliding_window: {c['sliding_window']}")
