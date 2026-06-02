"""Chunk newspaper text for vector indexing (metadata preserved)."""

from __future__ import annotations

import hashlib
import re
from typing import Any


def chunk_text(
    text: str,
    *,
    max_chars: int = 1200,
    overlap: int = 150,
) -> list[str]:
    """Split on paragraph boundaries with overlap."""
    if not text:
        return []
    paras = [p.strip() for p in re.split(r"\n{2,}|\.\s+", text) if len(p.strip()) > 40]
    if not paras:
        paras = [text[i : i + max_chars] for i in range(0, len(text), max_chars - overlap)]

    chunks: list[str] = []
    buf = ""
    for p in paras:
        if len(buf) + len(p) + 2 <= max_chars:
            buf = f"{buf} {p}".strip() if buf else p
        else:
            if buf:
                chunks.append(buf)
            buf = p
    if buf:
        chunks.append(buf)

    # Add sliding overlap for long single paragraphs
    out: list[str] = []
    for c in chunks:
        if len(c) <= max_chars:
            out.append(c)
            continue
        i = 0
        while i < len(c):
            out.append(c[i : i + max_chars])
            i += max_chars - overlap
    return out


def make_vector_records(
    mention: dict[str, Any],
    geo: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    """Build chunk records ready for embedding / Pinecone / Milvus ingest."""
    text = mention.get("text_snippet") or mention.get("article_title") or ""
    chunks = chunk_text(text)
    records = []
    for i, chunk in enumerate(chunks):
        chunk_id = hashlib.sha256(
            f"{mention.get('article_id')}:{i}:{chunk[:80]}".encode()
        ).hexdigest()[:24]
        records.append(
            {
                "id": f"chnk_{chunk_id}",
                "text": chunk,
                "metadata": {
                    "vessel_name": mention.get("vessel_name"),
                    "article_date": mention.get("article_date"),
                    "article_title": mention.get("article_title"),
                    "article_url": mention.get("article_url"),
                    "state": mention.get("state"),
                    "query": mention.get("query"),
                    "loss_keywords": mention.get("loss_keywords"),
                    "lat": geo.get("lat") if geo else None,
                    "lon": geo.get("lon") if geo else None,
                    "place_label": geo.get("label") if geo else None,
                    "geo_confidence": geo.get("confidence") if geo else None,
                    "chunk_index": i,
                    "source": "chronicling_america",
                },
            }
        )
    return records
