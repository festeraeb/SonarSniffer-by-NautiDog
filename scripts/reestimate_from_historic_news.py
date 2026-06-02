#!/usr/bin/env python3
"""
Match historic newspaper mentions to Swayze/estimated wrecks; propose better coordinates;
export drift-analysis candidate list.

Reads: outputs/historic_news/mentions.jsonl
Writes: outputs/historic_news/reestimated_wrecks.json
        outputs/historic_news/drift_candidates.json
Optional --apply: outputs/historic_news/location_updates.sql + table historic_news_mentions

Usage:
  python3 scripts/reestimate_from_historic_news.py
  python3 scripts/reestimate_from_historic_news.py --apply
  python3 scripts/reestimate_from_historic_news.py --min-confidence 0.5
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
MENTIONS = REPO / "outputs" / "historic_news" / "mentions.jsonl"
DB = Path(__import__("os").environ.get("DB_PATH", str(REPO / "db" / "wrecks.db")))
OUT_DIR = REPO / "outputs" / "historic_news"

# Prefer estimated rows for newspaper refinement
TARGET_PIN_CLASSES = ("estimated", "parsed", "unknown")
TARGET_QUALITIES = ("place_estimated", "lake_center", "agent_estimated", "swayze_parsed", "none")


def normalize_name(name: str) -> str:
    n = (name or "").upper()
    n = re.sub(r"\b(SS|MV|THE|A|AN)\b", "", n)
    n = re.sub(r"[^A-Z0-9\s]", "", n)
    return re.sub(r"\s+", " ", n).strip()


def haversine_m(a: tuple[float, float], b: tuple[float, float]) -> float:
    lat1, lon1 = a
    lat2, lon2 = b
    r = 6371000.0
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dp = math.radians(lat2 - lat1)
    dl = math.radians(lon2 - lon1)
    x = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * r * math.asin(math.sqrt(min(1.0, x)))


def load_mentions(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def load_db_features(conn: sqlite3.Connection) -> list[dict]:
    conn.row_factory = sqlite3.Row
    q = """
        SELECT id, name, latitude, longitude, date, coord_quality, pin_class,
               record_tier, historical_place_names, feature_type
        FROM features
        WHERE latitude IS NOT NULL
          AND (pin_class IN ('estimated','parsed','unknown')
               OR coord_quality IN ('place_estimated','lake_center','agent_estimated','swayze_parsed','none'))
    """
    return [dict(r) for r in conn.execute(q)]


def match_mention_to_features(mention: dict, features: list[dict]) -> list[dict]:
    vkey = normalize_name(mention.get("vessel_name", ""))
    if len(vkey) < 3:
        return []
    hits = []
    for f in features:
        fkey = normalize_name(f.get("name", ""))
        if not fkey:
            continue
        if vkey == fkey or vkey in fkey or fkey in vkey:
            hits.append(f)
    return hits


def score_improvement(mention: dict, feature: dict) -> dict | None:
    geo = mention.get("geo") or {}
    lat, lon = geo.get("lat"), geo.get("lon")
    conf = float(geo.get("confidence") or 0)
    if lat is None or lon is None or conf < 0.4:
        return None

    old_lat, old_lon = feature["latitude"], feature["longitude"]
    dist_m = haversine_m((old_lat, old_lon), (lat, lon))
    cq = feature.get("coord_quality") or ""

    # Skip tiny moves unless low-quality prior
    if dist_m < 3000 and cq not in ("lake_center", "none", "agent_estimated"):
        return None

    priority = "high" if dist_m > 15000 or cq == "lake_center" else "medium"
    if mention.get("loss_keywords"):
        priority = "high"

    return {
        "feature_id": feature["id"],
        "vessel_name": feature.get("name"),
        "old_lat": old_lat,
        "old_lon": old_lon,
        "new_lat": lat,
        "new_lon": lon,
        "shift_km": round(dist_m / 1000, 1),
        "coord_quality": "historic_news",
        "pin_class": "estimated",
        "geo_confidence": conf,
        "place_label": geo.get("label"),
        "geo_method": geo.get("method"),
        "article_url": mention.get("article_url"),
        "article_date": mention.get("article_date"),
        "article_title": mention.get("article_title"),
        "text_snippet": (mention.get("text_snippet") or "")[:400],
        "drift_priority": priority,
        "loss_date": mention.get("article_date") or feature.get("date"),
    }


def to_drift_candidate(row: dict) -> dict:
    """Format for historical_drift.py / CESAROPS case studies."""
    return {
        "case_name": row["vessel_name"],
        "feature_id": row["feature_id"],
        "departure": {
            "lat": row["new_lat"],
            "lon": row["new_lon"],
            "label": row.get("place_label") or "historic_news_estimate",
            "date": row.get("loss_date"),
        },
        "anchors": [
            {
                "lat": row["old_lat"],
                "lon": row["old_lon"],
                "label": "prior_swayze_estimate",
                "role": "prior",
            },
            {
                "lat": row["new_lat"],
                "lon": row["new_lon"],
                "label": row.get("place_label") or "news_geocode",
                "role": "last_seen",
            },
        ],
        "drift_priority": row["drift_priority"],
        "evidence_url": row.get("article_url"),
        "notes": (
            f"Historic news re-estimate; shifted {row['shift_km']} km from "
            f"{row.get('coord_quality', 'prior')} pin. Run forward/backward drift from last_seen."
        ),
    }


def apply_to_db(conn: sqlite3.Connection, updates: list[dict]) -> None:
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS historic_news_mentions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            feature_id INTEGER,
            vessel_name TEXT,
            article_url TEXT,
            article_date TEXT,
            new_lat REAL,
            new_lon REAL,
            geo_confidence REAL,
            place_label TEXT,
            drift_priority TEXT,
            applied INTEGER DEFAULT 0,
            created_at TEXT
        );
        """
    )
    now = datetime.now(timezone.utc).isoformat()
    for u in updates:
        conn.execute(
            """
            INSERT INTO historic_news_mentions (
                feature_id, vessel_name, article_url, article_date,
                new_lat, new_lon, geo_confidence, place_label,
                drift_priority, created_at
            ) VALUES (?,?,?,?,?,?,?,?,?,?)
            """,
            (
                u["feature_id"],
                u["vessel_name"],
                u.get("article_url"),
                u.get("article_date"),
                u["new_lat"],
                u["new_lon"],
                u.get("geo_confidence"),
                u.get("place_label"),
                u.get("drift_priority"),
                now,
            ),
        )
    conn.commit()


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--apply", action="store_true", help="Record rows in historic_news_mentions table")
    p.add_argument("--min-confidence", type=float, default=0.45)
    p.add_argument("--mentions", type=Path, default=MENTIONS)
    p.add_argument("--db", type=Path, default=DB)
    args = p.parse_args()

    mentions = load_mentions(args.mentions)
    if not mentions:
        raise SystemExit(
            f"No mentions at {args.mentions}. Run: python3 scripts/harvest_chronicling_america.py --quick"
        )

    conn = sqlite3.connect(args.db)
    features = load_db_features(conn)

    proposals: list[dict] = []
    seen_features: set[int] = set()

    for m in mentions:
        if float((m.get("geo") or {}).get("confidence") or 0) < args.min_confidence:
            continue
        for f in match_mention_to_features(m, features):
            fid = f["id"]
            if fid in seen_features:
                continue
            prop = score_improvement(m, f)
            if prop:
                proposals.append(prop)
                seen_features.add(fid)

    proposals.sort(key=lambda x: (-{"high": 2, "medium": 1}.get(x["drift_priority"], 0), -x["shift_km"]))

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    reest_path = OUT_DIR / "reestimated_wrecks.json"
    drift_path = OUT_DIR / "drift_candidates.json"

    reest_path.write_text(json.dumps(proposals, indent=2, ensure_ascii=False), encoding="utf-8")
    drift_cases = [to_drift_candidate(p) for p in proposals if p["drift_priority"] == "high"]
    drift_path.write_text(
        json.dumps(
            {
                "generated_at": datetime.now(timezone.utc).isoformat(),
                "count": len(drift_cases),
                "candidates": drift_cases,
                "run_drift": (
                    "python3 backup/deploy/tools/pipelines/satellite/historical_drift.py "
                    "(import cases from drift_candidates.json)"
                ),
            },
            indent=2,
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )

    if args.apply and proposals:
        apply_to_db(conn, proposals)

    conn.close()

    print(f"Mentions loaded: {len(mentions)}")
    print(f"Re-estimate proposals: {len(proposals)}")
    print(f"High-priority drift candidates: {len(drift_cases)}")
    print(f"Wrote {reest_path}")
    print(f"Wrote {drift_path}")
    if proposals[:3]:
        print("\nSample:")
        for p in proposals[:3]:
            print(f"  {p['vessel_name']}: shift {p['shift_km']} km → {p.get('place_label')}")


if __name__ == "__main__":
    main()
