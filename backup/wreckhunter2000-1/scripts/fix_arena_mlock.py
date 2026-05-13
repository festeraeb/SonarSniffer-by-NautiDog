#!/usr/bin/env python3
"""Fix the arena.rs mlock call - libc::mlock returns i32, not Result."""
import re

path = "/codebase/wreckhunter2000-1/cesarops-inference/src/arena.rs"

with open(path, "r") as f:
    content = f.read()

# Replace the broken if let Err pattern with proper i32 check
content = content.replace(
    'if let Err(e) = unsafe {\n',
    'let _ret = unsafe {\n'
)
content = content.replace(
    '    } {\n        tracing::warn!("Failed to lock pages: {} (non-fatal)", e);\n    }',
    '    };\n    if _ret != 0 {\n        tracing::warn!("mlock failed (non-fatal, may need root)");\n    }'
)

with open(path, "w") as f:
    f.write(content)

print("Fixed arena.rs mlock call")
