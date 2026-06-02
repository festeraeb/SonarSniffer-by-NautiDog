#!/usr/bin/env python3
"""
Harvest Great Lakes shipwreck articles from Chronicling America (loc.gov API).

Writes:
  outputs/historic_news/articles.jsonl      — raw article hits + mentions
  outputs/historic_news/mentions.jsonl      — per-vessel extractions
  outputs/historic_news/chunks.jsonl        — vector-ready paragraph chunks
  outputs/historic_news/harvest_summary.json

Then run:
  python3 scripts/reestimate_from_historic_news.py
  python3 scripts/reestimate_from_historic_news.py --apply   # update DB candidates table

Examples:
  python3 scripts/harvest_chronicling_america.py --quick
  python3 scripts/harvest_chronicling_america.py --states michigan,wisconsin --max-pages 3
  python3 scripts/harvest_chronicling_america.py --retrospective
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "scripts"))

from historic_news.config import (  # noqa: E402
    DEFAULT_END_DATE,
    DEFAULT_START_DATE,
    GL_PORTS,
    GL_STATES,
    MAX_PAGES_PER_QUERY,
    RETROSPECTIVE_QUERIES,
    STATE_NEWSPAPERS,
    WRECK_KEYWORDS,
)
from historic_news.chunker import make_vector_records  # noqa: E402
from historic_news.extract import article_to_mentions, mention_to_dict  # noqa: E402
from historic_news.geocode_gl import geocode_text  # noqa: E402
from historic_news.loc_client import search_pages  # noqa: E402

OUT_DIR = REPO / "outputs" / "historic_news"


def build_queries(
    quick: bool, retrospective: bool, states: list[str]
) -> list[tuple[str, str | None, str | None]]:
    """(qs, partof_title, location_state) — state optional when using local paper."""
    queries: list[tuple[str, str | None, str | None]] = []
    if retrospective:
        for q in RETROSPECTIVE_QUERIES:
            queries.append((q, None, None))

    keywords = WRECK_KEYWORDS[:4] if quick else WRECK_KEYWORDS
    for kw in keywords:
        queries.append((f"{kw}", None, None))

    for state in states:
        for paper in STATE_NEWSPAPERS.get(state, [])[:3 if quick else 6]:
            for kw in ("foundered", "schooner lost", "steamer lost", "marine disaster"):
                queries.append((kw, paper, None))

    if not quick:
        for port in GL_PORTS[:12]:
            queries.append(("schooner OR steamer foundered", port, None))
    return queries


def main() -> None:
    p = argparse.ArgumentParser(description="Harvest LOC Chronicling America wreck articles")
    p.add_argument("--states", default=",".join(GL_STATES[:4]),
                   help="Comma-separated location_state values (default: first 4 GL states)")
    p.add_argument("--start-date", default=DEFAULT_START_DATE)
    p.add_argument("--end-date", default=DEFAULT_END_DATE)
    p.add_argument("--max-pages", type=int, default=MAX_PAGES_PER_QUERY)
    p.add_argument("--quick", action="store_true", help="Fewer queries (smoke test)")
    p.add_argument("--retrospective", action="store_true", help="Include early-wreck compilation queries")
    args = p.parse_args()

    states = [s.strip() for s in args.states.split(",") if s.strip()]
    queries = build_queries(args.quick, args.retrospective, states)

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    articles_path = OUT_DIR / "articles.jsonl"
    mentions_path = OUT_DIR / "mentions.jsonl"
    chunks_path = OUT_DIR / "chunks.jsonl"

    seen_ids: set[str] = set()
    n_articles = 0
    n_mentions = 0
    n_chunks = 0
    errors = 0

    with articles_path.open("w", encoding="utf-8") as fa, \
         mentions_path.open("w", encoding="utf-8") as fm, \
         chunks_path.open("w", encoding="utf-8") as fc:

        for qs, partof, loc_state in queries:
            state_label = loc_state or (states[0] if len(states) == 1 else "multi")
            print(f"\n[query] paper={partof!r} state={loc_state!r} qs={qs[:50]!r}...")
            count = 0
            search_states = [loc_state] if loc_state else states
            for state in search_states:
                for item in search_pages(
                    qs,
                    start_date=args.start_date,
                    end_date=args.end_date,
                    location_state=state if not partof else None,
                    partof_title=partof,
                    max_pages=args.max_pages,
                ):
                    if item.get("_error"):
                        errors += 1
                        print(f"  ERR {item.get('_error')}")
                        continue
                    iid = item.get("id") or ""
                    if iid in seen_ids:
                        continue
                    seen_ids.add(iid)
                    fa.write(
                        json.dumps(
                            {"item": item, "query": qs, "state": state, "partof_title": partof},
                            ensure_ascii=False,
                        )
                        + "\n",
                    )
                    n_articles += 1
                    count += 1

                    for mention in article_to_mentions(item, qs, state):
                        md = mention_to_dict(mention)
                        geo = geocode_text(mention.text_snippet)
                        if geo:
                            md["geo"] = {
                                "lat": geo.lat,
                                "lon": geo.lon,
                                "label": geo.label,
                                "confidence": geo.confidence,
                                "method": geo.method,
                            }
                        fm.write(json.dumps(md, ensure_ascii=False) + "\n")
                        n_mentions += 1
                        for rec in make_vector_records(md, md.get("geo")):
                            fc.write(json.dumps(rec, ensure_ascii=False) + "\n")
                            n_chunks += 1

            print(f"  → {count} new articles")

    summary = {
        "harvested_at": datetime.now(timezone.utc).isoformat(),
        "states": states,
        "queries_run": len(queries),
        "unique_articles": n_articles,
        "vessel_mentions": n_mentions,
        "vector_chunks": n_chunks,
        "api_errors": errors,
        "outputs": {
            "articles": str(articles_path),
            "mentions": str(mentions_path),
            "chunks": str(chunks_path),
        },
    }
    (OUT_DIR / "harvest_summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8"
    )
    print("\n=== Harvest complete ===")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
