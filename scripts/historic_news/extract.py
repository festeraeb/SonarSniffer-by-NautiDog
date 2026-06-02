"""Extract vessel names and wreck events from newspaper snippets."""

from __future__ import annotations

import re
from dataclasses import dataclass, asdict
from typing import Any

# Vessel name patterns (19th c. press style)
VESSEL_PATTERNS = [
    re.compile(
        r"\b(?:schooner|steamer|propeller|bark|brig|tug|barge|scow)"
        r"\s+([A-Z][A-Za-z0-9'.\- ]{2,40}?)(?:\s|,|\.|;|was|has|is|went|foundered|lost)",
        re.I,
    ),
    re.compile(
        r"\b(?:the\s+)?([A-Z][A-Za-z'.\-]{2,30})\s+"
        r"(?:foundered|capsized|sank|went down|was lost|is missing|overdue)",
        re.I,
    ),
]

DATE_PATTERNS = [
    re.compile(r"\b(\d{1,2})\s+(Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|"
               r"May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|"
               r"Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)\s+(\d{4})\b", re.I),
    re.compile(r"\b(\d{4})[-/](\d{1,2})[-/](\d{1,2})\b"),
]

MONTH_MAP = {
    "jan": 1, "january": 1, "feb": 2, "february": 2, "mar": 3, "march": 3,
    "apr": 4, "april": 4, "may": 5, "jun": 6, "june": 6, "jul": 7, "july": 7,
    "aug": 8, "august": 8, "sep": 9, "sept": 9, "september": 9,
    "oct": 10, "october": 10, "nov": 11, "november": 11, "dec": 12, "december": 12,
}


@dataclass
class WreckMention:
    vessel_name: str
    article_title: str
    article_date: str | None
    article_id: str
    article_url: str
    state: str | None
    text_snippet: str
    query: str
    loss_keywords: list[str]


VESSEL_BLOCKLIST = {
    "WERE", "HOURS", "FARM", "DRAGNETHANDBAG", "DELMITELY", "WEATHER",
    "IMAGE", "FINAL", "STAR", "NEWS", "PRESS", "TRIBUNE", "HERALD",
    "MISSING", "TOTAL", "LOSS", "ALL", "HANDS", "GALE", "LAKE",
}


def _clean_vessel(name: str) -> str:
    name = re.sub(r"\s+", " ", name.strip())
    name = re.sub(r"^(the|a|an)\s+", "", name, flags=re.I)
    return name.strip(" .,;:'\"")[:80]


def _valid_vessel(name: str) -> bool:
    if len(name) < 3:
        return False
    key = re.sub(r"[^A-Z]", "", name.upper())
    if key in VESSEL_BLOCKLIST:
        return False
    if name.isupper() and len(name) < 5:
        return False
    return True


def extract_vessel_names(text: str) -> list[str]:
    names: list[str] = []
    seen: set[str] = set()
    for pat in VESSEL_PATTERNS:
        for m in pat.finditer(text):
            n = _clean_vessel(m.group(1))
            key = n.upper()
            if len(n) < 3 or key in seen:
                continue
            if n.lower() in ("lake", "harbor", "city", "point", "bay", "river"):
                continue
            if not _valid_vessel(n):
                continue
            seen.add(key)
            names.append(n)
    return names[:8]


def parse_date_from_text(text: str) -> str | None:
    for pat in DATE_PATTERNS:
        m = pat.search(text)
        if not m:
            continue
        g = m.groups()
        if len(g) == 3 and g[2].isdigit() and len(g[2]) == 4:
            mo = MONTH_MAP.get(g[1].lower()[:3], 0) or MONTH_MAP.get(g[1].lower(), 0)
            if mo:
                return f"{g[2]}-{mo:02d}-{int(g[0]):02d}"
        if len(g) == 3 and len(g[0]) == 4:
            return f"{g[0]}-{int(g[1]):02d}-{int(g[2]):02d}"
    return None


def loss_keywords_in(text: str) -> list[str]:
    keys = []
    for kw in (
        "lost with all hands", "foundered", "capsized", "went down",
        "total loss", "marine disaster", "missing", "overdue", "gale",
        "white hurricane", "all hands",
    ):
        if kw.lower() in text.lower():
            keys.append(kw)
    return keys


def is_great_lakes_relevant(text: str, title: str = "") -> bool:
    from .config import GL_RELEVANCE_PATTERNS

    blob = f"{title} {text}".lower()
    if any(p in blob for p in GL_RELEVANCE_PATTERNS):
        return True
    # Local paper title heuristic
    for hint in (
        "detroit", "michigan", "milwaukee", "wisconsin", "cleveland", "ohio",
        "buffalo", "duluth", "marquette", "traverse city", "green bay", "erie",
        "chicago", "great lakes",
    ):
        if hint in title.lower():
            return True
    return False


def article_to_mentions(item: dict[str, Any], query: str, state: str | None) -> list[WreckMention]:
    if item.get("_error"):
        return []

    title = item.get("title") or ""
    desc = " ".join(item.get("description") or []) if isinstance(item.get("description"), list) else (item.get("description") or "")
    text = f"{title}. {desc}"
    if len(text) < 30:
        return []

    if not is_great_lakes_relevant(text, title):
        return []

    if not loss_keywords_in(text) and not any(
        k in text.lower() for k in ("schooner", "steamer", "propeller", "wreck", "sunk")
    ):
        return []

    item_id = item.get("id") or ""
    url = item_id if item_id.startswith("http") else f"https://www.loc.gov{item_id}"
    date = item.get("date")
    if isinstance(date, list):
        date = date[0] if date else None
    if not date:
        date = parse_date_from_text(text)

    vessels = extract_vessel_names(text)
    if not vessels:
        # Title-only fallback: "Loss of the Foo" style
        m = re.search(r"\b(?:loss of|wreck of|foundering of)\s+(?:the\s+)?([A-Z][A-Za-z'.\- ]{2,35})", title, re.I)
        if m:
            vessels = [_clean_vessel(m.group(1))]

    mentions = []
    for v in vessels:
        mentions.append(
            WreckMention(
                vessel_name=v,
                article_title=title[:200],
                article_date=str(date)[:10] if date else None,
                article_id=item_id,
                article_url=url,
                state=state,
                text_snippet=text[:1500],
                query=query,
                loss_keywords=loss_keywords_in(text),
            )
        )
    return mentions


def mention_to_dict(m: WreckMention) -> dict:
    return asdict(m)
