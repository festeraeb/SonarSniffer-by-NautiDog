"""Library of Congress Chronicling America search client (loc.gov JSON API)."""

from __future__ import annotations

import time
from typing import Any, Iterator
from urllib.parse import urlencode

import requests

from .config import LOC_COLLECTION, REQUEST_DELAY_SEC

SESSION = requests.Session()
SESSION.headers.update(
    {
        "User-Agent": (
            "CESARops-HistoricNews/1.0 (maritime SAR research; "
            "https://github.com/AceOmni/wreckhunter2000-1)"
        )
    }
)


def search_pages(
    qs: str,
    *,
    start_date: str | None = None,
    end_date: str | None = None,
    location_state: str | None = None,
    partof_title: str | None = None,
    max_pages: int = 5,
) -> Iterator[dict[str, Any]]:
    """
    Yield result records from paginated collection search.
    Each record is a newspaper page/item summary from loc.gov.
    """
    params: dict[str, Any] = {
        "fo": "json",
        "c": 100,
        "qs": qs,
        "dl": "page",
    }
    if start_date:
        params["start_date"] = start_date
    if end_date:
        params["end_date"] = end_date
    if location_state:
        params["location_state"] = location_state
    if partof_title:
        params["partof_title"] = partof_title

    url = f"{LOC_COLLECTION}?{urlencode(params)}"
    pages = 0

    while url and pages < max_pages:
        time.sleep(REQUEST_DELAY_SEC)
        try:
            resp = SESSION.get(url, timeout=60)
            resp.raise_for_status()
            data = resp.json()
        except (requests.RequestException, ValueError) as err:
            yield {"_error": str(err), "_url": url}
            time.sleep(3)
            break

        for item in data.get("results") or []:
            if item.get("id") and "collection" not in (item.get("original_format") or []):
                yield item

        url = (data.get("pagination") or {}).get("next")
        pages += 1


def fetch_item_text(item_id: str) -> str:
    """
    Fetch OCR/text for an item if exposed in item JSON.
    item_id like https://www.loc.gov/item/sn85026453/1885-11-20/ed-1/
    """
    if not item_id.startswith("http"):
        item_id = f"https://www.loc.gov{item_id}" if item_id.startswith("/") else item_id
    if "?" not in item_id:
        item_id = item_id.rstrip("/") + "/?fo=json"

    time.sleep(REQUEST_DELAY_SEC)
    try:
        resp = SESSION.get(item_id, timeout=45)
        resp.raise_for_status()
        data = resp.json()
    except (requests.RequestException, ValueError):
        return ""

    parts: list[str] = []
    for key in ("description", "notes", "summary", "title"):
        v = data.get(key)
        if isinstance(v, str):
            parts.append(v)
        elif isinstance(v, list):
            parts.extend(str(x) for x in v)

    # Some items expose fulltext in resources
    for res in data.get("resources") or []:
        if isinstance(res, dict):
            for k in ("caption", "text", "description"):
                if res.get(k):
                    parts.append(str(res[k]))

    return "\n".join(parts)
