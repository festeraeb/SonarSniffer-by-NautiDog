#!/usr/bin/env python3
"""
Phase-1 restructure of wrecks.db — separate concerns without losing data.

What this does (--apply):
  1. Backup db/wrecks.db → db/wrecks.db.bak-<timestamp>
  2. Add record_tier column (census | estimated | survey | supplemental)
  3. Fix mislabeled coord_quality (Swayze+estimated rows marked dive_verified)
  4. Rename ThunderBay dive_verified → survey_verified
  5. Create supplemental_wrecks table + load preserve scrape (GPS-verified)
  6. Create views: features_census, features_estimated, features_survey, features_public_map

Does NOT delete rows. Phase 2 (canonical dedup) is documented in docs/WRECK_DB_RESTRUCTURE.md.

Usage:
  python3 scripts/restructure_wrecks_db.py --dry-run
  python3 scripts/restructure_wrecks_db.py --apply
"""

from __future__ import annotations

import argparse
import json
import shutil
import sqlite3
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
DB = REPO / "db" / "wrecks.db"
PRESERVE_JSON = REPO / "outputs" / "great_lakes_preserve_wrecks.json"
LEGACY_WEB_JSON = REPO / "backup/wreckhunter2000-1/outputs/web_scraped_wrecks.json"

SUPPLEMENTAL_DDL = """
CREATE TABLE IF NOT EXISTS supplemental_wrecks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    latitude REAL NOT NULL,
    longitude REAL NOT NULL,
    depth_ft INTEGER,
    year_lost INTEGER,
    vessel_type TEXT,
    lake TEXT,
    state TEXT,
    preserve TEXT,
    coord_quality TEXT NOT NULL,
    source_url TEXT,
    source_site TEXT,
    imported_at TEXT NOT NULL,
    matched_feature_id INTEGER,
    match_distance_m REAL,
    UNIQUE(name, latitude, longitude, source_url)
);
"""

VIEWS_DDL = """
CREATE VIEW IF NOT EXISTS features_census AS
SELECT * FROM features
WHERE record_tier = 'census' OR (record_tier IS NULL AND source = 'Swayze2019-1.xlsx');

CREATE VIEW IF NOT EXISTS features_estimated AS
SELECT * FROM features
WHERE record_tier = 'estimated'
   OR (record_tier IS NULL AND source LIKE 'Swayze%estimated%');

CREATE VIEW IF NOT EXISTS features_survey AS
SELECT * FROM features
WHERE record_tier = 'survey'
   OR coord_quality IN ('survey_verified', 'dive_verified')
      AND source NOT LIKE 'Swayze%';

CREATE VIEW IF NOT EXISTS features_public_map AS
SELECT f.* FROM features f
WHERE f.coord_quality IN (
    'survey_verified', 'dive_verified', 'preserve_registry',
    'sanctuary_registry', 'gps', 'chart', 'mission_gt'
)
OR f.record_tier = 'survey'
OR EXISTS (
    SELECT 1 FROM supplemental_wrecks s
    WHERE s.matched_feature_id = f.id
);
"""


def backup_db(db: Path) -> Path:
    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    dest = db.with_suffix(f".db.bak-{ts}")
    shutil.copy2(db, dest)
    return dest


def column_exists(conn: sqlite3.Connection, table: str, col: str) -> bool:
    cur = conn.execute(f"PRAGMA table_info({table})")
    return col in {r[1] for r in cur.fetchall()}


def classify_tier(source: str | None, coord_quality: str | None) -> str:
    src = source or ""
    cq = coord_quality or ""
    if src == "ThunderBay_NOAA" or cq in ("survey_verified", "dive_verified") and not src.startswith("Swayze"):
        return "survey"
    if "+estimated" in src or cq in ("place_estimated", "lake_center", "agent_estimated"):
        return "estimated"
    if src.startswith("Swayze"):
        return "census"
    return "census"


def load_supplemental_json(path: Path) -> list[dict]:
    if not path.exists():
        return []
    data = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(data, dict) and "wrecks" in data:
        data = data["wrecks"]
    rows = []
    for w in data if isinstance(data, list) else []:
        lat = w.get("lat") or w.get("latitude")
        lon = w.get("lon") or w.get("longitude")
        if lat is None or lon is None:
            continue
        rows.append(w)
    return rows


def import_supplemental(conn: sqlite3.Connection, records: list[dict], dry_run: bool) -> int:
    if dry_run:
        return len(records)
    now = datetime.now(timezone.utc).isoformat()
    n = 0
    for w in records:
        try:
            cur = conn.execute(
                """
                INSERT OR IGNORE INTO supplemental_wrecks (
                    name, latitude, longitude, depth_ft, year_lost, vessel_type,
                    lake, state, preserve, coord_quality, source_url, source_site, imported_at
                ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
                """,
                (
                    w.get("name"),
                    float(w["lat"] if "lat" in w else w["latitude"]),
                    float(w["lon"] if "lon" in w else w["longitude"]),
                    w.get("depth_ft"),
                    w.get("year_lost"),
                    w.get("vessel_type"),
                    w.get("lake"),
                    w.get("state"),
                    w.get("preserve"),
                    w.get("coord_quality", "preserve_registry"),
                    w.get("source"),
                    w.get("source_site"),
                    now,
                ),
            )
            if cur.rowcount:
                n += 1
        except (KeyError, TypeError, ValueError):
            continue
    return n


def restructure(conn: sqlite3.Connection, dry_run: bool) -> dict:
    stats = {"fixes": {}, "supplemental_loaded": 0}

    if not dry_run:
        conn.executescript(SUPPLEMENTAL_DDL)

    # Add record_tier
    if not column_exists(conn, "features", "record_tier"):
        if not dry_run:
            conn.execute("ALTER TABLE features ADD COLUMN record_tier TEXT")
        stats["added_record_tier"] = True

    cur = conn.execute("SELECT id, source, coord_quality FROM features")
    rows = cur.fetchall()
    tier_updates = 0
    for rid, source, cq in rows:
        tier = classify_tier(source, cq)
        if not dry_run:
            conn.execute(
                "UPDATE features SET record_tier = ? WHERE id = ? AND (record_tier IS NULL OR record_tier = '')",
                (tier, rid),
            )
        tier_updates += 1
    stats["tier_classified"] = tier_updates

    # Fix mislabeled dive_verified on Swayze estimated rows
    fix_agent = """
        UPDATE features SET coord_quality = 'agent_estimated'
        WHERE coord_quality = 'dive_verified'
          AND source LIKE 'Swayze%estimated%'
    """
    if dry_run:
        n = conn.execute(
            "SELECT COUNT(*) FROM features WHERE coord_quality='dive_verified' AND source LIKE 'Swayze%estimated%'"
        ).fetchone()[0]
    else:
        conn.execute(fix_agent)
        n = conn.total_changes
    stats["fixes"]["swayze_estimated_dive_verified_to_agent_estimated"] = n

    # Thunder Bay → survey_verified
    fix_tb = """
        UPDATE features SET coord_quality = 'survey_verified', record_tier = 'survey'
        WHERE source = 'ThunderBay_NOAA' AND coord_quality = 'dive_verified'
    """
    if dry_run:
        n = conn.execute(
            "SELECT COUNT(*) FROM features WHERE source='ThunderBay_NOAA' AND coord_quality='dive_verified'"
        ).fetchone()[0]
    else:
        conn.execute(fix_tb)
        n = conn.total_changes
    stats["fixes"]["thunderbay_to_survey_verified"] = n

    # Remaining Swayze dive_verified (not +estimated) → swayze_parsed or place_estimated heuristics
    fix_swayze_dv = """
        UPDATE features SET coord_quality = 'swayze_parsed'
        WHERE coord_quality = 'dive_verified'
          AND source = 'Swayze2019-1.xlsx'
    """
    if dry_run:
        n = conn.execute(
            "SELECT COUNT(*) FROM features WHERE coord_quality='dive_verified' AND source='Swayze2019-1.xlsx'"
        ).fetchone()[0]
    else:
        conn.execute(fix_swayze_dv)
        n = conn.total_changes
    stats["fixes"]["swayze_raw_dive_verified_to_swayze_parsed"] = n

    records = load_supplemental_json(PRESERVE_JSON)
    if not records:
        records = load_supplemental_json(LEGACY_WEB_JSON)
    stats["supplemental_loaded"] = import_supplemental(conn, records, dry_run)

    if not dry_run:
        for stmt in VIEWS_DDL.strip().split(";"):
            s = stmt.strip()
            if s:
                conn.execute(s)

    if not dry_run:
        conn.commit()

    return stats


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--apply", action="store_true", help="Write changes (creates backup first)")
    p.add_argument("--dry-run", action="store_true", help="Report only (default)")
    p.add_argument("--db", type=Path, default=DB)
    args = p.parse_args()
    dry_run = not args.apply

    if not args.db.exists():
        raise SystemExit(f"Missing {args.db}")

    if args.apply:
        bak = backup_db(args.db)
        print(f"Backup: {bak}")

    conn = sqlite3.connect(args.db)
    try:
        stats = restructure(conn, dry_run=dry_run)
    finally:
        conn.close()

    mode = "DRY-RUN" if dry_run else "APPLIED"
    print(f"\n[{mode}] Restructure stats:")
    print(json.dumps(stats, indent=2))
    if dry_run:
        print("\nRe-run with --apply to execute (backs up DB first).")


if __name__ == "__main__":
    main()
