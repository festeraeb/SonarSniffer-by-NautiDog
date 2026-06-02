#!/usr/bin/env python3
"""
Inventory and optionally merge supplemental wreck catalogs into wrecks.db.

Sources (non-Swayze):
  - known_wrecks.json (mission GT bboxes)
  - erie_known_wrecks_db.py output CSV
  - web_scraped_wrecks.json

Usage:
  python scripts/merge_wreck_sources.py              # report only
  python scripts/merge_wreck_sources.py --dry-run  # show INSERTs
  python scripts/merge_wreck_sources.py --apply    # write supplemental rows

Does not delete Swayze rows. New rows use source tags and coord_quality.
"""
from __future__ import annotations

import argparse
import json
import sqlite3
import subprocess
import sys
from pathlib import Path

import os

REPO = Path(__file__).resolve().parents[1]
DB = Path(os.environ.get("DB_PATH", str(REPO / "db" / "wrecks.db")))

KNOWN_JSON = REPO / "backup/deploy/tools/cesarops-core-github/known_wrecks.json"
WEB_SCRAPED = REPO / "outputs/great_lakes_preserve_wrecks.json"
LEGACY_WEB = REPO / "backup/wreckhunter2000-1/outputs/web_scraped_wrecks.json"
ERIE_SCRIPT = REPO / "projects/pipelines/mag/erie_known_wrecks_db.py"
if not ERIE_SCRIPT.exists():
    ERIE_SCRIPT = Path("/codebase/projects/pipelines/mag/erie_known_wrecks_db.py")


def count_db(path: Path) -> dict:
    if not path.exists():
        return {"error": "missing"}
    conn = sqlite3.connect(path)
    c = conn.cursor()
    c.execute("SELECT COUNT(*) FROM features")
    total = c.fetchone()[0]
    c.execute("SELECT COUNT(*) FROM features WHERE latitude IS NOT NULL")
    with_coords = c.fetchone()[0]
    c.execute("SELECT source, COUNT(*) FROM features GROUP BY source ORDER BY COUNT(*) DESC LIMIT 15")
    by_source = dict(c.fetchall())
    conn.close()
    return {"total": total, "with_coords": with_coords, "top_sources": by_source}


def load_known_json() -> list[dict]:
    if not KNOWN_JSON.exists():
        return []
    data = json.loads(KNOWN_JSON.read_text())
    out = []
    for _k, v in data.items():
        if isinstance(v, dict) and "lat_min" in v:
            lat = (float(v["lat_min"]) + float(v["lat_max"])) / 2
            lon = (float(v["lon_min"]) + float(v["lon_max"])) / 2
            out.append(
                {
                    "name": v.get("name", "unknown"),
                    "latitude": lat,
                    "longitude": lon,
                    "source": "mission_gt",
                    "coord_quality": "mission_gt",
                }
            )
    return out


def load_web_scraped() -> list[dict]:
    path = WEB_SCRAPED if WEB_SCRAPED.exists() else LEGACY_WEB
    if not path.exists():
        return []
    data = json.loads(path.read_text())
    if isinstance(data, dict) and "wrecks" in data:
        data = data["wrecks"]
    out = []
    for w in data if isinstance(data, list) else []:
        lat = w.get("latitude") or w.get("lat")
        lon = w.get("longitude") or w.get("lon")
        if lat is None or lon is None:
            continue
        out.append(
            {
                "name": w.get("name", "unknown"),
                "latitude": float(lat),
                "longitude": float(lon),
                "source": w.get("source", "web_scraped"),
                "coord_quality": w.get("coord_quality", "preserve_registry"),
            }
        )
    return out


def load_erie_csv() -> list[dict]:
    import tempfile

    if not ERIE_SCRIPT.exists():
        return []
    out_csv = Path(tempfile.gettempdir()) / "erie_known_wrecks_all.csv"
    subprocess.run(
        [sys.executable, str(ERIE_SCRIPT), "--output", str(out_csv)],
        check=False,
        capture_output=True,
    )
    if not out_csv.exists():
        return []
    import csv

    rows = []
    with out_csv.open() as f:
        for row in csv.DictReader(f):
            try:
                rows.append(
                    {
                        "name": row["name"],
                        "latitude": float(row["lat"]),
                        "longitude": float(row["lon"]),
                        "source": f"erie_{row.get('source', 'nda')}",
                        "coord_quality": row.get("coord_quality", "gps"),
                    }
                )
            except (KeyError, ValueError):
                continue
    return rows


def report() -> None:
    print("=== Wreck database inventory ===\n")
    print(f"Primary DB: {DB}")
    print(json.dumps(count_db(DB), indent=2))
    for label, loader in [
        ("known_wrecks.json", load_known_json),
        ("web_scraped", load_web_scraped),
        ("erie CSV", load_erie_csv),
    ]:
        rows = loader()
        print(f"\n{label}: {len(rows)} records (not yet merged unless --apply)")


def apply_merge(dry_run: bool) -> None:
    supplements = load_known_json() + load_web_scraped() + load_erie_csv()
    print(f"Supplemental rows to consider: {len(supplements)}")
    if dry_run or not supplements:
        for r in supplements[:5]:
            print(" ", r)
        if len(supplements) > 5:
            print(f"  ... and {len(supplements) - 5} more")
        return
    # Full merge deferred — requires dedup vs Swayze by name+proximity
    print("Use --apply after dedup logic is implemented (see docs/WRECK_DATABASE_INVENTORY.md)")


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--apply", action="store_true")
    args = p.parse_args()
    report()
    if args.apply or args.dry_run:
        apply_merge(dry_run=not args.apply)


if __name__ == "__main__":
    main()
