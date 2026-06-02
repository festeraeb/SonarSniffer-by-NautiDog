"""Persist historic newspaper articles and wreck matches in wrecks.db."""

from __future__ import annotations

import sqlite3
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[2]
DEFAULT_DB = REPO / "db" / "wrecks.db"

SCHEMA_SQL = """
CREATE TABLE IF NOT EXISTS historic_news_articles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    loc_id TEXT UNIQUE,
    article_url TEXT NOT NULL UNIQUE,
    title TEXT,
    published_date TEXT,
    newspaper TEXT,
    ocr_text TEXT,
    search_query TEXT,
    harvest_source TEXT DEFAULT 'chronicling_america',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS historic_news_matches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feature_id INTEGER NOT NULL,
    article_id INTEGER NOT NULL,
    is_match INTEGER NOT NULL DEFAULT 0,
    match_confidence REAL,
    llm_reasoning TEXT,
    estimated_lat REAL,
    estimated_lon REAL,
    place_evidence TEXT,
    date_consistent INTEGER,
    shipping_lane_note TEXT,
    drift_candidate INTEGER DEFAULT 0,
    coord_quality TEXT DEFAULT 'historic_news_llm',
    created_at TEXT NOT NULL,
    UNIQUE(feature_id, article_id),
    FOREIGN KEY (article_id) REFERENCES historic_news_articles(id)
);

CREATE INDEX IF NOT EXISTS idx_hn_matches_feature ON historic_news_matches(feature_id);
CREATE INDEX IF NOT EXISTS idx_hn_matches_match ON historic_news_matches(is_match, match_confidence);
"""


def connect(db_path: Path | str = DEFAULT_DB) -> sqlite3.Connection:
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    return conn


def init_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(SCHEMA_SQL)
    conn.commit()


def upsert_article(conn: sqlite3.Connection, article: dict[str, Any]) -> int:
    """Insert article; return article id."""
    now = datetime.now(timezone.utc).isoformat()
    loc_id = article.get("loc_id") or article.get("article_url")
    conn.execute(
        """
        INSERT INTO historic_news_articles (
            loc_id, article_url, title, published_date, newspaper,
            ocr_text, search_query, harvest_source, created_at
        ) VALUES (?,?,?,?,?,?,?,?,?)
        ON CONFLICT(article_url) DO UPDATE SET
            title=excluded.title,
            ocr_text=excluded.ocr_text,
            published_date=excluded.published_date
        """,
        (
            loc_id,
            article["article_url"],
            article.get("title"),
            article.get("published_date"),
            article.get("newspaper"),
            article.get("ocr_text"),
            article.get("search_query"),
            article.get("harvest_source", "chronicling_america"),
            now,
        ),
    )
    conn.commit()
    row = conn.execute(
        "SELECT id FROM historic_news_articles WHERE article_url = ?",
        (article["article_url"],),
    ).fetchone()
    return int(row["id"])


def save_match(
    conn: sqlite3.Connection,
    feature_id: int,
    article_id: int,
    result: dict[str, Any],
) -> None:
    now = datetime.now(timezone.utc).isoformat()
    is_match = 1 if result.get("is_match") else 0
    conf = float(result.get("match_confidence") or 0)
    drift = 1 if result.get("drift_candidate") and is_match else 0
    conn.execute(
        """
        INSERT INTO historic_news_matches (
            feature_id, article_id, is_match, match_confidence, llm_reasoning,
            estimated_lat, estimated_lon, place_evidence, date_consistent,
            shipping_lane_note, drift_candidate, coord_quality, created_at
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
        ON CONFLICT(feature_id, article_id) DO UPDATE SET
            is_match=excluded.is_match,
            match_confidence=excluded.match_confidence,
            llm_reasoning=excluded.llm_reasoning,
            estimated_lat=excluded.estimated_lat,
            estimated_lon=excluded.estimated_lon,
            place_evidence=excluded.place_evidence,
            date_consistent=excluded.date_consistent,
            shipping_lane_note=excluded.shipping_lane_note,
            drift_candidate=excluded.drift_candidate
        """,
        (
            feature_id,
            article_id,
            is_match,
            conf,
            result.get("llm_reasoning"),
            result.get("estimated_lat"),
            result.get("estimated_lon"),
            result.get("place_evidence"),
            1 if result.get("date_consistent") else 0,
            result.get("shipping_lane_note"),
            drift,
            result.get("coord_quality", "historic_news_llm"),
            now,
        ),
    )
    conn.commit()


def apply_feature_update(
    conn: sqlite3.Connection,
    feature_id: int,
    lat: float,
    lon: float,
    *,
    min_confidence: float = 0.72,
    match_confidence: float,
) -> bool:
    """Update features row when LLM match is strong enough."""
    if match_confidence < min_confidence:
        return False
    conn.execute(
        """
        UPDATE features SET
            latitude = ?,
            longitude = ?,
            coord_quality = 'historic_news_llm',
            pin_class = 'estimated',
            record_tier = 'estimated'
        WHERE id = ?
        """,
        (lat, lon, feature_id),
    )
    conn.commit()
    return conn.total_changes > 0


def stats(conn: sqlite3.Connection) -> dict[str, int]:
    def q(sql: str) -> int:
        return conn.execute(sql).fetchone()[0]

    return {
        "articles": q("SELECT COUNT(*) FROM historic_news_articles"),
        "matches_total": q("SELECT COUNT(*) FROM historic_news_matches"),
        "matches_positive": q("SELECT COUNT(*) FROM historic_news_matches WHERE is_match=1"),
        "drift_candidates": q(
            "SELECT COUNT(*) FROM historic_news_matches WHERE drift_candidate=1"
        ),
    }
