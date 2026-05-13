#!/usr/bin/env python3
"""Fix gasket for kernel 6.17 - use bare token MODULE_IMPORT_NS"""
import pathlib

p = pathlib.Path("/tmp/gasket-driver/src/gasket_page_table.c")
t = p.read_text()

# Replace quoted version with bare token
t = t.replace('MODULE_IMPORT_NS("DMA_BUF");', 'MODULE_IMPORT_NS(DMA_BUF);')

p.write_text(t)
print("Fixed: MODULE_IMPORT_NS(DMA_BUF) bare token")

# Verify
for i, line in enumerate(t.split('\n')):
    if 'MODULE_IMPORT_NS' in line:
        print(f"  Line {i+1}: {line.strip()}")
