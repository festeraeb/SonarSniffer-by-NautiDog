#!/usr/bin/env python3
"""Audit wrecks.db — Swayze census vs estimated coords vs true surveys."""

from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
DB = Path(__import__("os").environ.get("DB_PATH", str(REPO / "db" / "wrecks.db")))


def audit(db_path: Path) -> dict:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    c = conn.cursor()

    def q(sql: str, params=()):
        c.execute(sql, params)
        return c.fetchall()

    total = q("SELECT COUNT(*) n FROM features")[0]["n"]
    with_lat = q("SELECT COUNT(*) n FROM features WHERE latitude IS NOT NULL")[0]["n"]

    by_source = {r["source"]: r["n"] for r in q(
        "SELECT source, COUNT(*) n FROM features GROUP BY source ORDER BY n DESC"
    )}
    by_quality = {r["coord_quality"]: r["n"] for r in q(
        "SELECT coord_quality, COUNT(*) n FROM features GROUP BY coord_quality ORDER BY n DESC"
    )}

    mislabeled_dv = q("""
        SELECT COUNT(*) n FROM features
        WHERE coord_quality = 'dive_verified'
          AND source LIKE 'Swayze%'
    """)[0]["n"]

    placeholder_dv = q("""
        SELECT COUNT(*) n FROM features
        WHERE coord_quality = 'dive_verified'
          AND (
            ABS(latitude - 43.5) < 0.02 OR ABS(latitude - 44.0) < 0.02
            OR ABS(longitude + 77.5) < 0.02 OR ABS(longitude + 87.0) < 0.02
            OR (latitude = ROUND(latitude, 1) AND longitude = ROUND(longitude, 1))
          )
    """)[0]["n"]

    dup_names = q("""
        SELECT COUNT(*) n FROM (
          SELECT UPPER(TRIM(name)) nm FROM features
          WHERE name IS NOT NULL AND TRIM(name) != ''
          GROUP BY nm HAVING COUNT(*) > 1
        )
    """)[0]["n"]

    dup_rows = q("""
        SELECT SUM(cnt) - COUNT(*) n FROM (
          SELECT UPPER(TRIM(name)) nm, COUNT(*) cnt FROM features
          WHERE name IS NOT NULL GROUP BY nm HAVING cnt > 1
        )
    """)[0]["n"] or 0

    true_survey = q("""
        SELECT COUNT(*) n FROM features
        WHERE source IN ('ThunderBay_NOAA')
           OR (coord_quality = 'dive_verified' AND source NOT LIKE 'Swayze%')
    """)[0]["n"]

    supplemental_in_features = q("""
        SELECT COUNT(*) n FROM features
        WHERE coord_quality IN ('preserve_registry', 'gps', 'mission_gt', 'sanctuary_registry')
           OR source LIKE '%preserve%' OR source LIKE '%erie_%'
    """)[0]["n"]

    has_tier = False
    c.execute("PRAGMA table_info(features)")
    cols = {r[1] for r in c.fetchall()}
    if "record_tier" in cols:
        has_tier = True
        by_tier = {r["record_tier"]: r["n"] for r in q(
            "SELECT record_tier, COUNT(*) n FROM features GROUP BY record_tier"
        )}
    else:
        by_tier = {}

    c.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    tables = [r[0] for r in c.fetchall()]

    conn.close()

    return {
        "db": str(db_path),
        "total_features": total,
        "with_coordinates": with_lat,
        "tables": tables,
        "by_source": by_source,
        "by_coord_quality": by_quality,
        "by_record_tier": by_tier,
        "has_record_tier_column": has_tier,
        "issues": {
            "swayze_labeled_dive_verified": mislabeled_dv,
            "dive_verified_placeholder_coords": placeholder_dv,
            "duplicate_name_groups": dup_names,
            "extra_rows_from_duplicates": dup_rows,
            "likely_true_survey_rows": true_survey,
            "supplemental_quality_in_features": supplemental_in_features,
        },
        "interpretation": {
            "swayze_census_rows": by_source.get("Swayze2019-1.xlsx", 0)
                + by_source.get("Swayze2019-1.xlsx+estimated", 0),
            "estimated_coords": (
                by_quality.get("place_estimated", 0)
                + by_quality.get("lake_center", 0)
            ),
            "parsed_from_swayze_text": by_quality.get("swayze_parsed", 0),
        },
    }


def main() -> None:
    db = Path(sys.argv[1]) if len(sys.argv) > 1 else DB
    if not db.exists():
        print(f"Missing {db}", file=sys.stderr)
        sys.exit(1)

    report = audit(db)
    print(json.dumps(report, indent=2))

    issues = report["issues"]
    print("\n--- Summary ---")
    print(f"Rows in `features`: {report['total_features']}")
    print(f"Swayze census (both sources): {report['interpretation']['swayze_census_rows']}")
    print(f"Estimated coords (place + lake center): {report['interpretation']['estimated_coords']}")
    print(f"Mislabeled Swayze as dive_verified: {issues['swayze_labeled_dive_verified']}")
    print(f"Duplicate name groups: {issues['duplicate_name_groups']}")
    print(f"True survey-ish rows (Thunder Bay etc.): {issues['likely_true_survey_rows']}")
    print(f"Preserve/Erie rows in features: {issues['supplemental_quality_in_features']}")
    print("\nRun: python3 scripts/restructure_wrecks_db.py --dry-run")
    print("     python3 scripts/restructure_wrecks_db.py --apply")


if __name__ == "__main__":
    main()
