#!/usr/bin/env python3
"""
Resumable batch: Chronicling America harvest + RTX 2060 LLM match + save to wrecks.db.

Designed for cesarops2 (:5200 llama-server on CUDA0 / RTX 2060 SUPER).
Processes 10–20 estimated Swayze wrecks per batch; checkpoints after each batch.

Usage (on cesarops2):
  bash scripts/cesarops2_research_lab.sh start   # :5200 coder on 2060
  python3 scripts/batch_wreck_historic_news.py --batch-size 15
  python3 scripts/batch_wreck_historic_news.py --resume
  python3 scripts/batch_wreck_historic_news.py --resume --save   # default: saves matches

Env:
  LLM_URL=http://127.0.0.1:5200/v1/chat/completions
  DB_PATH=/mnt/t440/codebase/repos/wreckhunter2000-1/db/wrecks.db
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "scripts"))

from historic_news.db import (  # noqa: E402
    apply_feature_update,
    connect,
    init_schema,
    save_match,
    stats,
    upsert_article,
)
from historic_news.geocode_gl import geocode_text  # noqa: E402
from historic_news.loc_client import fetch_item_text, search_pages  # noqa: E402
from historic_news.shipping_lanes import lanes_context_block, nearest_lane_hint  # noqa: E402

OUT_DIR = REPO / "outputs" / "historic_news"
STATE_FILE = OUT_DIR / "batch_llm_state.json"
DEFAULT_DB = Path(__import__("os").environ.get("DB_PATH", str(REPO / "db" / "wrecks.db")))
DEFAULT_LLM = __import__("os").environ.get(
    "LLM_URL", "http://127.0.0.1:5200/v1/chat/completions"
)


def parse_wreck_year(date_str: str | None) -> tuple[str | None, str | None]:
    """Return (start_date, end_date) YYYY-MM-DD for LOC search window."""
    if not date_str:
        return None, None
    m = re.search(r"(\d{4})", str(date_str))
    if not m:
        return None, None
    y = int(m.group(1))
    if y < 1600 or y > 1950:
        return None, None
    return f"{y - 3}-01-01", f"{y + 2}-12-31"


def item_to_article_dict(item: dict[str, Any], query: str) -> dict[str, Any]:
    title = item.get("title") or ""
    desc = item.get("description")
    if isinstance(desc, list):
        desc = " ".join(str(x) for x in desc)
    desc = desc or ""
    loc_id = item.get("id") or ""
    url = loc_id if str(loc_id).startswith("http") else f"https://www.loc.gov{loc_id}"
    date = item.get("date")
    if isinstance(date, list):
        date = date[0] if date else None
    ocr = f"{title}\n{desc}".strip()
    if len(ocr) < 400:
        extra = fetch_item_text(url)
        if extra:
            ocr = f"{ocr}\n{extra}"[:12000]
    newspaper = title.split(",")[-1].strip() if "," in title else None
    return {
        "loc_id": loc_id,
        "article_url": url,
        "title": title[:500],
        "published_date": str(date)[:10] if date else None,
        "newspaper": newspaper,
        "ocr_text": ocr[:12000],
        "search_query": query,
    }


def harvest_articles_for_wreck(wreck: dict, max_articles: int = 6) -> list[dict]:
    name = wreck.get("name") or ""
    if len(name) < 2:
        return []
    qs = f'"{name}" (schooner OR steamer OR propeller) (foundered OR lost OR wreck)'
    start, end = parse_wreck_year(wreck.get("date"))
    if not start:
        start, end = "1850-01-01", "1924-12-31"

    articles: list[dict] = []
    seen: set[str] = set()
    for item in search_pages(qs, start_date=start, end_date=end, max_pages=1):
        if item.get("_error"):
            continue
        url = item.get("id") or ""
        if url in seen:
            continue
        seen.add(url)
        articles.append(item_to_article_dict(item, qs))
        if len(articles) >= max_articles:
            break
    return articles


def load_state() -> dict:
    if STATE_FILE.exists():
        return json.loads(STATE_FILE.read_text(encoding="utf-8"))
    return {
        "version": 1,
        "processed_feature_ids": [],
        "stats": {"batches": 0, "wrecks": 0, "articles_saved": 0, "matches_saved": 0},
    }


def save_state(state: dict) -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    state["updated_at"] = datetime.now(timezone.utc).isoformat()
    STATE_FILE.write_text(json.dumps(state, indent=2), encoding="utf-8")


def load_wreck_batch(
    conn: sqlite3.Connection,
    batch_size: int,
    processed: set[int],
) -> list[dict]:
    placeholders = ",".join("?" * len(processed)) if processed else ""
    extra = f"AND id NOT IN ({placeholders})" if processed else ""
    params: list[Any] = list(processed) if processed else []
    params.append(batch_size)
    sql = f"""
        SELECT id, name, date, latitude, longitude, feature_type,
               historical_place_names, coord_quality, pin_class, source
        FROM features
        WHERE latitude IS NOT NULL
          AND pin_class IN ('estimated', 'parsed', 'unknown')
          AND (coord_quality IN (
                'place_estimated', 'lake_center', 'agent_estimated',
                'swayze_parsed', 'none', 'historic_news', 'historic_news_llm'
              ) OR coord_quality IS NULL)
          {extra}
        ORDER BY id
        LIMIT ?
    """
    return [dict(r) for r in conn.execute(sql, params)]


def call_llm(
    llm_url: str,
    wreck: dict,
    articles: list[dict],
    *,
    timeout: int = 180,
) -> dict[str, Any]:
    try:
        import requests
    except ImportError as exc:
        raise SystemExit("pip install requests") from exc

    article_blocks = []
    for i, a in enumerate(articles):
        article_blocks.append(
            f"--- Article {i} ---\n"
            f"URL: {a['article_url']}\n"
            f"Date: {a.get('published_date')}\n"
            f"Title: {a.get('title')}\n"
            f"Text:\n{(a.get('ocr_text') or '')[:2500]}\n"
        )

    wreck_ctx = (
        f"Vessel: {wreck.get('name')}\n"
        f"Swayze loss date field: {wreck.get('date')}\n"
        f"Type: {wreck.get('feature_type')}\n"
        f"Historical places (Swayze): {wreck.get('historical_place_names')}\n"
        f"Current estimate: {wreck.get('latitude')}, {wreck.get('longitude')} "
        f"({wreck.get('coord_quality')})\n"
    )
    geo_hint = geocode_text(str(wreck.get("historical_place_names") or ""))
    if geo_hint:
        wreck_ctx += (
            f"Place geocode hint: {geo_hint.label} ({geo_hint.lat}, {geo_hint.lon}) "
            f"conf={geo_hint.confidence}\n"
        )
    lane_hint = None
    if wreck.get("latitude") and wreck.get("longitude"):
        lane_hint = nearest_lane_hint(wreck["latitude"], wreck["longitude"])
    if lane_hint:
        wreck_ctx += f"{lane_hint}\n"

    system = (
        "You are a Great Lakes maritime historian and SAR analyst. "
        "Decide if newspaper articles describe the SAME vessel loss as the wreck record. "
        "Use dates, vessel name variants, route, and geography. "
        "Use shipping lanes and historical place names. "
        "Respond with ONLY valid JSON, no markdown.\n\n"
        + lanes_context_block()
    )
    user = (
        f"{wreck_ctx}\n\n"
        f"Articles to evaluate:\n\n"
        + "\n".join(article_blocks)
        + "\n\n"
        "Return JSON:\n"
        "{\n"
        '  "best_article_index": null or 0-based index,\n'
        '  "is_match": true/false,\n'
        '  "match_confidence": 0.0-1.0,\n'
        '  "date_consistent": true/false,\n'
        '  "estimated_lat": number or null,\n'
        '  "estimated_lon": number or null,\n'
        '  "place_evidence": "short quote or place name",\n'
        '  "shipping_lane_note": "which lane or corridor if relevant",\n'
        '  "drift_candidate": true if good for drift/backtrack analysis,\n'
        '  "reasoning": "2-4 sentences"\n'
        "}"
    )

    payload = {
        "model": "local",
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": 0.15,
        "max_tokens": 800,
    }
    resp = requests.post(llm_url, json=payload, timeout=timeout)
    resp.raise_for_status()
    content = resp.json()["choices"][0]["message"]["content"]
    return parse_llm_json(content)


def parse_llm_json(text: str) -> dict[str, Any]:
    text = text.strip()
    m = re.search(r"\{[\s\S]*\}", text)
    if not m:
        return {"is_match": False, "match_confidence": 0, "llm_reasoning": text[:500]}
    try:
        return json.loads(m.group(0))
    except json.JSONDecodeError:
        return {"is_match": False, "match_confidence": 0, "llm_reasoning": text[:500]}


def process_batch(
    wrecks: list[dict],
    conn: sqlite3.Connection,
    llm_url: str,
    *,
    save: bool,
    max_articles: int,
    min_confidence: float,
    apply_coords: bool,
) -> dict[str, int]:
    counts = {"wrecks": 0, "articles_saved": 0, "matches_saved": 0, "coords_updated": 0}

    for wreck in wrecks:
        fid = wreck["id"]
        print(f"\n[wreck {fid}] {wreck.get('name')}")
        articles = harvest_articles_for_wreck(wreck, max_articles=max_articles)
        print(f"  LOC articles: {len(articles)}")
        if not articles:
            continue

        try:
            llm_out = call_llm(llm_url, wreck, articles)
        except Exception as err:
            print(f"  LLM ERROR: {err}")
            llm_out = {"is_match": False, "match_confidence": 0, "llm_reasoning": str(err)}

        is_match = bool(llm_out.get("is_match"))
        conf = float(llm_out.get("match_confidence") or 0)
        idx = llm_out.get("best_article_index")
        print(f"  match={is_match} conf={conf:.2f} idx={idx}")

        if not save:
            counts["wrecks"] += 1
            continue

        if is_match and conf >= min_confidence:
            try:
                idx = int(idx) if idx is not None else 0
            except (TypeError, ValueError):
                idx = 0
            idx = max(0, min(idx, len(articles) - 1))
            aid = upsert_article(conn, articles[idx])
            counts["articles_saved"] += 1
            result = {
                "is_match": True,
                "match_confidence": conf,
                "llm_reasoning": llm_out.get("reasoning"),
                "estimated_lat": llm_out.get("estimated_lat"),
                "estimated_lon": llm_out.get("estimated_lon"),
                "place_evidence": llm_out.get("place_evidence"),
                "date_consistent": llm_out.get("date_consistent"),
                "shipping_lane_note": llm_out.get("shipping_lane_note"),
                "drift_candidate": llm_out.get("drift_candidate"),
            }
            save_match(conn, fid, aid, result)
            counts["matches_saved"] += 1
            lat, lon = result.get("estimated_lat"), result.get("estimated_lon")
            if apply_coords and lat is not None and lon is not None:
                if apply_feature_update(
                    conn, fid, float(lat), float(lon), match_confidence=conf
                ):
                    counts["coords_updated"] += 1
                    print(f"  updated feature → {lat}, {lon}")

        counts["wrecks"] += 1
        time.sleep(0.5)

    return counts


def main() -> None:
    p = argparse.ArgumentParser(description="Batch LOC + LLM wreck matching (resumable)")
    p.add_argument("--batch-size", type=int, default=15)
    p.add_argument("--max-articles", type=int, default=6, help="LOC pages per wreck")
    p.add_argument("--resume", action="store_true", help="Continue from batch_llm_state.json")
    p.add_argument("--no-save", action="store_true", help="Dry-run LLM only (no DB writes)")
    p.add_argument("--apply-coords", action="store_true", help="Update features lat/lon on strong match")
    p.add_argument("--min-confidence", type=float, default=0.65)
    p.add_argument("--llm-url", default=DEFAULT_LLM)
    p.add_argument("--db", type=Path, default=DEFAULT_DB)
    p.add_argument("--max-batches", type=int, default=0, help="0 = until queue empty")
    args = p.parse_args()
    save = not args.no_save

    state = load_state() if args.resume else {
        "version": 1,
        "processed_feature_ids": [],
        "stats": {"batches": 0, "wrecks": 0, "articles_saved": 0, "matches_saved": 0},
    }
    processed = set(state.get("processed_feature_ids") or [])

    conn = connect(args.db)
    init_schema(conn)

    batch_num = 0
    while True:
        wrecks = load_wreck_batch(conn, args.batch_size, processed)
        if not wrecks:
            print("\nQueue empty — all targeted wrecks processed.")
            break
        batch_num += 1
        print(f"\n{'='*60}\nBATCH {batch_num} — {len(wrecks)} wrecks (ids {wrecks[0]['id']}–{wrecks[-1]['id']})\n{'='*60}")

        counts = process_batch(
            wrecks,
            conn,
            args.llm_url,
            save=save,
            max_articles=args.max_articles,
            min_confidence=args.min_confidence,
            apply_coords=args.apply_coords,
        )

        for w in wrecks:
            processed.add(w["id"])
        state["processed_feature_ids"] = sorted(processed)
        st = state.setdefault("stats", {})
        st["batches"] = st.get("batches", 0) + 1
        for k, v in counts.items():
            st[k] = st.get(k, 0) + v
        save_state(state)
        print(f"\nCheckpoint saved → {STATE_FILE}")
        print(f"DB stats: {stats(conn)}")

        if args.max_batches and batch_num >= args.max_batches:
            print(f"Stopped after --max-batches {args.max_batches}")
            break

    conn.close()
    print("\nDone.")


if __name__ == "__main__":
    main()
