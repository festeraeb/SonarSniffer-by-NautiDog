#!/usr/bin/env python3
"""Patch cluster_config.toml [[gpu]] rows with gpu_uuid / bus_id from Forge /cluster/gpus."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import requests

REPO = Path(__file__).resolve().parents[2]
CONFIG = REPO / "cesarops-forge-v2" / "cluster_config.toml"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--forge-url", default="http://127.0.0.1:9100")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    r = requests.get(f"{args.forge_url.rstrip('/')}/cluster/gpus", timeout=30)
    r.raise_for_status()
    gpus = r.json().get("gpus") or []

    text = CONFIG.read_text(encoding="utf-8")
    updated = 0
    for g in gpus:
        uuid = (g.get("gpu_uuid") or "").strip()
        if not uuid or uuid.startswith("config-"):
            continue
        node = g.get("node") or ""
        gpu_id = g.get("id")
        pci = (g.get("pci_bus_id") or "").strip()
        # Match [[gpu]] block by id = N under same file (sequential patch)
        pattern = rf"(\[\[gpu\]\][^\[]*?id\s*=\s*{gpu_id}\s*\n)"
        block_m = re.search(pattern, text, re.S)
        if not block_m:
            continue
        block = block_m.group(1)
        if f'gpu_uuid = "{uuid}"' in block:
            continue
        insert_lines = []
        if pci and f'bus_id = "{pci}"' not in block and "bus_id =" not in block:
            insert_lines.append(f'bus_id = "{pci}"\n')
        insert_lines.append(f'gpu_uuid = "{uuid}"\n')
        new_block = block.rstrip() + "\n" + "".join(insert_lines)
        text = text.replace(block, new_block, 1)
        updated += 1
        print(f"  id={gpu_id} node={node} → {uuid[:20]}…")

    if args.dry_run:
        print(f"dry-run: would update {updated} gpu rows")
        return
    if updated:
        CONFIG.write_text(text, encoding="utf-8")
        print(f"Wrote {updated} gpu_uuid entries to {CONFIG}")
    else:
        print("No changes needed.")


if __name__ == "__main__":
    main()
