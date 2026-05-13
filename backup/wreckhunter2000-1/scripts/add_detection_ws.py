#!/usr/bin/env python3
import pathlib, re
p = pathlib.Path("/home/cesarops/wreckhunter2000-1/Cargo.toml")
t = p.read_text()
if "cesarops-detection" not in t:
    t = re.sub(r'(members = \[.*?)"cesarops-thought-engine"', r'\1"cesarops-thought-engine", "cesarops-detection"', t)
    p.write_text(t)
    print("Added cesarops-detection to workspace")
else:
    print("Already in workspace")
