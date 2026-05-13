#!/usr/bin/env python3
"""
Quick smoke-test for drive_discovery.  Run on any node:
  python scripts/_test_drive_discovery.py
"""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent.parent))

from drive_discovery import find_armor_drive

print("=== WreckHunter Drive Discovery Test ===\n")
result = find_armor_drive(quiet=False)

if result:
    print(f"\n✓ Drive found at: {result}")
    db = result / "LAKE_MICHIGAN_CENSUS_2026.db"
    dl = result / "downloads"
    print(f"  DB:        {db}  {'EXISTS' if db.exists() else 'MISSING'}")
    print(f"  downloads: {dl}  {'EXISTS' if dl.exists() else 'MISSING'}")
    if dl.exists():
        tifs = list(dl.rglob("*.tif")) + list(dl.rglob("*.TIF"))
        print(f"  TIF count: {len(tifs)}")
else:
    print("\n✗ Drive not found.")
    sys.exit(1)
