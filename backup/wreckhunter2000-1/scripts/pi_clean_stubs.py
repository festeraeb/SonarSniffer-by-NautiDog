"""
pi_clean_stubs.py — Remove mypy types-* stub packages from Pi system Python.
These are IDE-only stubs with no runtime function. Safe to remove.
Runs on the Pi directly: sudo python3 /tmp/pi_clean_stubs.py
"""
import os
import shutil

SITE_DIR = "/usr/lib/python3/dist-packages"

with open("/tmp/types_pkgs.txt") as f:
    pkgs = set(line.strip().lower().replace("-", "_") for line in f if line.strip())

removed = []
errors = []
skipped = []

for entry in sorted(os.listdir(SITE_DIR)):
    full_path = os.path.join(SITE_DIR, entry)
    low = entry.lower()

    # Match dist-info dirs: types_aiofiles-24.1.dist-info
    if low.endswith(".dist-info"):
        # Strip version: types_aiofiles-24.1  -> types_aiofiles
        name_ver = low[: -len(".dist-info")]          # e.g. types_aiofiles-24.1
        name = name_ver.rsplit("-", 1)[0]             # e.g. types_aiofiles
        if name in pkgs:
            try:
                shutil.rmtree(full_path)
                removed.append(entry)
            except Exception as e:
                errors.append(f"{entry}: {e}")
        else:
            skipped.append(name)

    # Match stub data dirs: e.g. aiofiles-stubs, pywintypes-stubs
    elif low.endswith("-stubs") or low.endswith("_stubs"):
        suffix = "-stubs" if low.endswith("-stubs") else "_stubs"
        guess = low[: -len(suffix)]
        if ("types_" + guess) in pkgs or guess in pkgs:
            try:
                shutil.rmtree(full_path)
                removed.append(entry)
            except Exception as e:
                errors.append(f"{entry}: {e}")

print(f"Removed {len(removed)} entries")
if errors:
    print("Errors:")
    for e in errors:
        print("  ERR:", e)
