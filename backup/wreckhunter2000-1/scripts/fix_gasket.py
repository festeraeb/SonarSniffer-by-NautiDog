#!/usr/bin/env python3
"""Fix gasket driver for kernel 6.17"""
import pathlib

# Fix gasket_page_table.c
p = pathlib.Path("/tmp/gasket-driver/src/gasket_page_table.c")
t = p.read_text()
# Remove any broken MODULE_IMPORT_NS lines
lines = t.split('\n')
clean_lines = []
for line in lines:
    if 'MODULE_IMPORT_NS' in line:
        continue  # skip all existing attempts
    clean_lines.append(line)

# Find MODULE_LICENSE line and add after it
for i, line in enumerate(clean_lines):
    if 'MODULE_LICENSE' in line:
        clean_lines.insert(i + 1, 'MODULE_IMPORT_NS("DMA_BUF");')
        break

p.write_text('\n'.join(clean_lines))
print("Fixed gasket_page_table.c")

# Verify gasket_core.c
p2 = pathlib.Path("/tmp/gasket-driver/src/gasket_core.c")
t2 = p2.read_text()
if 'no_llseek' in t2:
    t2 = t2.replace('no_llseek', 'noop_llseek')
    p2.write_text(t2)
    print("Fixed gasket_core.c")
else:
    print("gasket_core.c OK")
