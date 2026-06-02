#!/usr/bin/env python3
"""
Phase 2: canonical wreck sites + pin_class for map layers.

Creates:
  - pin_class on features and supplemental_wrecks (verified | estimated | parsed | unknown)
  - wreck_canonical — one best pin per normalized vessel name cluster
  - wreck_canonical_members — links features/supplemental rows to canonical

Usage:
  python3 scripts/wreck_db_phase2.py --dry-run
  python3 scripts/wreck_db_phase2.py --apply
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sqlite3
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
DB = REPO / "db" / "wrecks.db"

# Higher = better position trust
QUALITY_RANK = {
    "preserve_registry": 100,
    "sanctuary_registry": 100,
    "survey_verified": 95,
    "gps": 90,
    "chart": 88,
    "mission_gt": 85,
    "dive_verified": 80,
    "swayze_parsed": 50,
    "place_estimated": 30,
    "agent_estimated": 25,
    "lake_center": 20,
    "none": 5,
    "unknown": 0,
}

PIN_CLASS_FOR_QUALITY = {
    "preserve_registry": "verified",
    "sanctuary_registry": "verified",
    "survey_verified": "verified",
    "gps": "verified",
    "chart": "verified",
    "mission_gt": "verified",
    "dive_verified": "verified",
    "swayze_parsed": "parsed",
    "place_estimated": "estimated",
    "agent_estimated": "estimated",
    "lake_center": "estimated",
    "none": "unknown",
}


def normalize_name(name: str) -> str:
    n = (name or "").upper()
    n = re.sub(r"\b(SS|MV|THE|A|AN)\b", "", n)
    n = re.sub(r"[^A-Z0-9\s]", "", n)
    return re.sub(r"\s+", " ", n).strip()


def haversine_m(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    r = 6371000.0
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dp = math.radians(lat2 - lat1)
    dl = math.radians(lon2 - lon1)
    a = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * r * math.asin(math.sqrt(min(1.0, a)))


DDL = """
CREATE TABLE IF NOT EXISTS wreck_canonical (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_key TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    best_lat REAL NOT NULL,
    best_lon REAL NOT NULL,
    best_pin_class TEXT NOT NULL,
    best_coord_quality TEXT,
    best_feature_id INTEGER,
    best_supplemental_id INTEGER,
    member_count INTEGER DEFAULT 1,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS wreck_canonical_members (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_id INTEGER NOT NULL,
    feature_id INTEGER,
    supplemental_id INTEGER,
    pin_class TEXT,
    coord_quality TEXT,
    FOREIGN KEY (canonical_id) REFERENCES wreck_canonical(id)
);

"""


def column_exists(conn: sqlite3.Connection, table: str, col: str) -> bool:
    cur = conn.execute(f"PRAGMA table_info({table})")
    return col in {r[1] for r in cur.fetchall()}


def ensure_pin_class_columns(conn: sqlite3.Connection, dry_run: bool) -> None:
    for table in ("features", "supplemental_wrecks"):
        if not column_exists(conn, table, "pin_class") and not dry_run:
            conn.execute(f"ALTER TABLE {table} ADD COLUMN pin_class TEXT")


def assign_pin_classes(conn: sqlite3.Connection, dry_run: bool) -> dict:
    stats = {"features": 0, "supplemental": 0}
    if dry_run:
        cur = conn.execute(
            "SELECT coord_quality, COUNT(*) FROM features WHERE latitude IS NOT NULL GROUP BY coord_quality"
        )
        return {"features_by_quality": dict(cur.fetchall())}

    for cq, pc in PIN_CLASS_FOR_QUALITY.items():
        conn.execute(
            "UPDATE features SET pin_class = ? WHERE coord_quality = ? OR (coord_quality IS NULL AND ? = 'unknown')",
            (pc, cq, pc),
        )
    conn.execute(
        "UPDATE features SET pin_class = 'unknown' WHERE pin_class IS NULL AND latitude IS NOT NULL"
    )
    stats["features"] = conn.execute(
        "SELECT COUNT(*) FROM features WHERE pin_class IS NOT NULL"
    ).fetchone()[0]

    for cq, pc in PIN_CLASS_FOR_QUALITY.items():
        conn.execute(
            "UPDATE supplemental_wrecks SET pin_class = ? WHERE coord_quality = ?",
            (pc, cq),
        )
    conn.execute(
        "UPDATE supplemental_wrecks SET pin_class = 'verified' WHERE pin_class IS NULL"
    )
    stats["supplemental"] = conn.execute("SELECT COUNT(*) FROM supplemental_wrecks").fetchone()[0]
    return stats


def match_supplemental_to_features(conn: sqlite3.Connection, dry_run: bool) -> int:
    if dry_run:
        return conn.execute("SELECT COUNT(*) FROM supplemental_wrecks").fetchone()[0]

    rows = conn.execute(
        "SELECT id, name, latitude, longitude FROM supplemental_wrecks"
    ).fetchall()
    matched = 0
    for sid, name, lat, lon in rows:
        key = normalize_name(name)
        if not key:
            continue
        # Same name, prefer closest within 3 km
        cands = conn.execute(
            """
            SELECT id, latitude, longitude FROM features
            WHERE latitude IS NOT NULL
              AND UPPER(TRIM(name)) = UPPER(TRIM(?))
            """,
            (name,),
        ).fetchall()
        best_id, best_d = None, 1e18
        for fid, flat, flon in cands:
            d = haversine_m(lat, lon, flat, flon)
            if d < best_d:
                best_d, best_id = d, fid
        if best_id is not None and best_d <= 3000:
            sup_cq = conn.execute(
                "SELECT coord_quality FROM supplemental_wrecks WHERE id = ?", (sid,)
            ).fetchone()[0]
            conn.execute(
                "UPDATE supplemental_wrecks SET matched_feature_id = ?, match_distance_m = ? WHERE id = ?",
                (best_id, round(best_d, 1), sid),
            )
            matched += 1
            conn.execute(
                """
                UPDATE features SET
                    latitude = ?, longitude = ?,
                    coord_quality = ?, pin_class = 'verified',
                    record_tier = 'survey'
                WHERE id = ? AND (
                    coord_quality IN ('place_estimated', 'lake_center', 'agent_estimated', 'swayze_parsed', 'none')
                    OR coord_quality IS NULL
                )
                """,
                (lat, lon, sup_cq, best_id),
            )
    return matched


def build_canonical(conn: sqlite3.Connection, dry_run: bool) -> dict:
    """One canonical site per normalized name (best-ranked position)."""
    if dry_run:
        c = conn.execute(
            """
            SELECT COUNT(*) FROM (
              SELECT 1 FROM features WHERE latitude IS NOT NULL
              GROUP BY UPPER(TRIM(name))
            )
            """
        ).fetchone()[0]
        return {"canonical_groups_estimate": c}

    if not dry_run:
        conn.execute("DELETE FROM wreck_canonical_members")
        conn.execute("DELETE FROM wreck_canonical")

    # Candidates: features with coords + supplemental
    candidates: list[dict] = []
    for row in conn.execute(
        """
        SELECT id, name, latitude, longitude, coord_quality, pin_class, depth, source
        FROM features WHERE latitude IS NOT NULL AND name IS NOT NULL AND TRIM(name) != ''
        """
    ):
        candidates.append(
            {
                "feature_id": row[0],
                "supplemental_id": None,
                "name": row[1],
                "lat": row[2],
                "lon": row[3],
                "coord_quality": row[4] or "unknown",
                "pin_class": row[5] or PIN_CLASS_FOR_QUALITY.get(row[4], "unknown"),
                "depth": row[6],
                "source": row[7],
            }
        )
    for row in conn.execute(
        """
        SELECT id, name, latitude, longitude, coord_quality, pin_class, depth_ft, preserve, source_url
        FROM supplemental_wrecks
        """
    ):
        candidates.append(
            {
                "feature_id": None,
                "supplemental_id": row[0],
                "name": row[1],
                "lat": row[2],
                "lon": row[3],
                "coord_quality": row[4] or "preserve_registry",
                "pin_class": row[5] or "verified",
                "depth": row[6],
                "source": row[8],
                "preserve": row[7],
            }
        )

    groups: dict[str, list[dict]] = {}
    for c in candidates:
        key = normalize_name(c["name"])
        if len(key) < 2:
            continue
        groups.setdefault(key, []).append(c)

    now = datetime.now(timezone.utc).isoformat()
    canonical_count = 0
    member_count = 0

    for key, members in groups.items():
        if len(members) == 1 and members[0]["pin_class"] == "unknown":
            continue

        def rank(m: dict) -> tuple:
            q = m.get("coord_quality") or "unknown"
            return (QUALITY_RANK.get(q, 0), 1 if m.get("supplemental_id") else 0)

        best = max(members, key=rank)
        display = best["name"]
        conn.execute(
            """
            INSERT INTO wreck_canonical (
                canonical_key, display_name, best_lat, best_lon,
                best_pin_class, best_coord_quality,
                best_feature_id, best_supplemental_id, member_count, updated_at
            ) VALUES (?,?,?,?,?,?,?,?,?,?)
            """,
            (
                key,
                display,
                best["lat"],
                best["lon"],
                best["pin_class"],
                best.get("coord_quality"),
                best.get("feature_id"),
                best.get("supplemental_id"),
                len(members),
                now,
            ),
        )
        cid = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
        canonical_count += 1
        for m in members:
            conn.execute(
                """
                INSERT INTO wreck_canonical_members (
                    canonical_id, feature_id, supplemental_id, pin_class, coord_quality
                ) VALUES (?,?,?,?,?)
                """,
                (
                    cid,
                    m.get("feature_id"),
                    m.get("supplemental_id"),
                    m.get("pin_class"),
                    m.get("coord_quality"),
                ),
            )
            member_count += 1

    return {"canonical_sites": canonical_count, "members_linked": member_count}


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--apply", action="store_true")
    p.add_argument("--db", type=Path, default=DB)
    args = p.parse_args()
    dry_run = not args.apply

    conn = sqlite3.connect(args.db)
    try:
        if not dry_run:
            conn.executescript(DDL)
        ensure_pin_class_columns(conn, dry_run)
        if not dry_run:
            for stmt in (
                "CREATE INDEX IF NOT EXISTS idx_features_pin_class ON features(pin_class)",
                "CREATE INDEX IF NOT EXISTS idx_features_lat_lon ON features(latitude, longitude)",
                "CREATE INDEX IF NOT EXISTS idx_supplemental_pin ON supplemental_wrecks(pin_class)",
            ):
                try:
                    conn.execute(stmt)
                except sqlite3.OperationalError:
                    pass
        s1 = assign_pin_classes(conn, dry_run)
        s2 = match_supplemental_to_features(conn, dry_run)
        s3 = build_canonical(conn, dry_run)
        if not dry_run:
            conn.commit()
    finally:
        conn.close()

    mode = "DRY-RUN" if dry_run else "APPLIED"
    print(f"[{mode}]", json.dumps({"pin_class": s1, "supplemental_matched": s2, "canonical": s3}, indent=2))


if __name__ == "__main__":
    main()
