#!/usr/bin/env python3
"""
Scrape Great Lakes underwater preserve / sanctuary wreck registries.

Sources (polite delay between requests):
  - https://www.michiganpreserves.org/  (+ 13 preserve subsites)
  - Wikipedia Michigan preserve pages (+ West Michigan)
  - https://thunderbay.noaa.gov/shipwrecks/  (individual wreck pages)
  - Wikipedia Wisconsin Shipwreck Coast NMS list

Output:
  outputs/great_lakes_preserve_wrecks.json
  outputs/great_lakes_preserve_wrecks.csv
  outputs/great_lakes_preserve_summary.txt

Usage:
  python3 scripts/scrape_great_lakes_preserves.py
  python3 scripts/scrape_great_lakes_preserves.py --fast   # skip NOAA per-wreck pages
"""

from __future__ import annotations

import argparse
import csv
import json
import re
import time
from pathlib import Path
from urllib.parse import urljoin, urlparse

try:
    import requests
    from bs4 import BeautifulSoup
except ImportError as exc:
    raise SystemExit("pip install requests beautifulsoup4") from exc

REPO = Path(__file__).resolve().parents[1]
OUT_JSON = REPO / "outputs" / "great_lakes_preserve_wrecks.json"
OUT_CSV = REPO / "outputs" / "great_lakes_preserve_wrecks.csv"
OUT_SUMMARY = REPO / "outputs" / "great_lakes_preserve_summary.txt"

HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (compatible; CESARops-WreckScraper/2.0; "
        "maritime-research; +https://github.com/AceOmni/wreckhunter2000-1)"
    )
}
DELAY = 1.0

MI_PRESERVES_BASE = "https://www.michiganpreserves.org"
THUNDERBAY_INDEX = "https://thunderbay.noaa.gov/shipwrecks/"
THUNDERBAY_BASE = "https://thunderbay.noaa.gov"

WIKI_MI_PRESERVES = [
    "https://en.wikipedia.org/wiki/Straits_of_Mackinac_Shipwreck_Preserve",
    "https://en.wikipedia.org/wiki/Whitefish_Point_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/Keweenaw_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/Alger_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/De_Tour_Passage_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/Grand_Traverse_Bay_Bottomland_Preserve",
    "https://en.wikipedia.org/wiki/Manitou_Passage_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/Marquette_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/Sanilac_Shores_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/Southwest_Michigan_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/Thumb_Area_Bottomland_Preserve",
    "https://en.wikipedia.org/wiki/West_Michigan_Underwater_Preserve",
    "https://en.wikipedia.org/wiki/Thunder_Bay_National_Marine_Sanctuary",
]

WIKI_WI_SANCTUARY = (
    "https://en.wikipedia.org/wiki/"
    "List_of_shipwrecks_in_the_Wisconsin_Shipwreck_Coast_National_Marine_Sanctuary"
)

_session = requests.Session()
_session.headers.update(HEADERS)


def fetch(url: str, retries: int = 3) -> str | None:
    for attempt in range(retries):
        try:
            r = _session.get(url, timeout=25)
            r.raise_for_status()
            return r.text
        except requests.RequestException as err:
            print(f"  [WARN] {url} ({attempt + 1}/{retries}): {err}")
            time.sleep(2**attempt)
    return None


_request_delay = DELAY


def polite_get(url: str) -> str | None:
    time.sleep(_request_delay)
    return fetch(url)


def dms_to_dd(deg: float, minutes: float, seconds: float = 0.0) -> float:
    return deg + minutes / 60.0 + seconds / 3600.0


_PAT_A = re.compile(
    r"(\d{1,3})\xb0(\d{1,2}(?:\.\d+)?)[′']\s*N\s+"
    r"0?(\d{1,3})\xb0(\d{1,2}(?:\.\d+)?)[′']\s*W",
    re.IGNORECASE,
)
_PAT_B = re.compile(
    r"N\s+(\d{1,3})\xb0\s+(\d{1,2}(?:\.\d+)?)\s+W\s+0?(\d{1,3})\xb0\s+(\d{1,2}(?:\.\d+)?)",
    re.IGNORECASE,
)
_PAT_C = re.compile(
    r"(\d{1,3})°(\d{1,2}(?:\.\d+)?)['′]\s*N\s+"
    r"(\d{1,3})°(\d{1,2}(?:\.\d+)?)['′]\s*W",
    re.IGNORECASE,
)
_PAT_DECIMAL = re.compile(
    r"(\d{1,2})°(\d{1,2})['′]?\s*(\d{1,2})?[″\"]?\s*([NS])\s+"
    r"(\d{1,3})°(\d{1,2})['′]?\s*(\d{1,2})?[″\"]?\s*([EW])",
    re.IGNORECASE,
)
_PAT_DD = re.compile(
    r"(-?\d{1,2}\.\d+)\s*°?\s*,?\s*(-?\d{2,3}\.\d+)\s*°?",
)


def parse_coords(text: str) -> tuple[float, float] | None:
    for pat in (_PAT_A, _PAT_B, _PAT_C):
        m = pat.search(text)
        if m:
            d1, m1, d2, m2 = (float(x) for x in m.groups())
            lat, lon = dms_to_dd(d1, m1), -dms_to_dd(d2, m2)
            if 40 <= lat <= 50 and -95 <= lon <= -75:
                return round(lat, 6), round(lon, 6)

    m = _PAT_DECIMAL.search(text)
    if m:
        lat_d, lat_m, lat_s, lat_h, lon_d, lon_m, lon_s, lon_h = m.groups()
        lat = dms_to_dd(float(lat_d), float(lat_m), float(lat_s or 0))
        lon = dms_to_dd(float(lon_d), float(lon_m), float(lon_s or 0))
        if lat_h.upper() == "S":
            lat = -lat
        if lon_h.upper() == "W":
            lon = -lon
        if 40 <= lat <= 50 and -95 <= lon <= -75:
            return round(lat, 6), round(lon, 6)

    m = _PAT_DD.search(text)
    if m:
        lat, lon = float(m.group(1)), float(m.group(2))
        if lon > 0:
            lon = -lon
        if 40 <= lat <= 50 and -95 <= lon <= -75:
            return round(lat, 6), round(lon, 6)
    return None


_PAT_DEPTH = re.compile(r"(\d+)\s*(?:to\s*\d+)?\s*['′]?\s*ft", re.IGNORECASE)
_YEAR_IN_NAME = re.compile(r"\((\d{4})\)")


def parse_depth_ft(text: str) -> int | None:
    m = _PAT_DEPTH.search(text)
    return int(m.group(1)) if m else None


def parse_year_from_name(name: str) -> int | None:
    m = _YEAR_IN_NAME.search(name)
    return int(m.group(1)) if m else None


def clean_name(name: str) -> str:
    name = re.sub(r"\s+", " ", name.strip())
    return name


def normalize_name(name: str) -> str:
    name = name.upper()
    name = re.sub(r"\b(SS|MV|THE|A|AN)\b", "", name)
    name = re.sub(r"[^A-Z0-9\s]", "", name)
    return re.sub(r"\s+", " ", name).strip()


def infer_lake(lat: float, lon: float) -> str:
    if lat >= 46.4 and lon <= -84.4:
        return "Lake Superior"
    if lat >= 45.5 and -86.0 <= lon <= -83.5:
        return "Lake Huron"
    if lon <= -84.5 and lat < 46.4:
        return "Lake Huron"
    if -87.5 <= lon <= -76.0 and lat < 45.5:
        return "Lake Michigan"
    if lon >= -83.0 and lat < 43.0:
        return "Lake Erie"
    return "Great Lakes"


def discover_michigan_preserves() -> list[str]:
    html = polite_get(MI_PRESERVES_BASE)
    if not html:
        return []
    soup = BeautifulSoup(html, "html.parser")
    urls: set[str] = set()
    for a in soup.find_all("a", href=True):
        href = a["href"]
        if any(
            k in href.lower()
            for k in ("underwater-preserve", "bottomland", "shipwreck-preserve")
        ):
            urls.add(urljoin(MI_PRESERVES_BASE, href))
    return sorted(urls)


def scrape_michiganpreserves_page(url: str) -> list[dict]:
    print(f"\n[michiganpreserves.org] {url}")
    html = polite_get(url)
    if not html:
        return []

    soup = BeautifulSoup(html, "html.parser")
    preserve = soup.find("h1")
    preserve_name = preserve.get_text(strip=True) if preserve else url.rstrip("/").split("/")[-1]

    results: list[dict] = []
    seen: set[str] = set()

    for table in soup.find_all("table"):
        rows = table.find_all("tr")
        if len(rows) < 2:
            continue
        header = rows[0].get_text(" ", strip=True).lower()
        if "wreck" not in header and "gps" not in header and "lat" not in header:
            continue

        for row in rows[1:]:
            cells = row.find_all(["td", "th"])
            if len(cells) < 2:
                continue
            raw_name = cells[0].get_text(" ", strip=True)
            if not raw_name or raw_name.lower().startswith("wreck name"):
                continue

            row_text = " ".join(c.get_text(" ", strip=True) for c in cells)
            coords = parse_coords(row_text)
            if not coords:
                continue

            lat, lon = coords
            depth = parse_depth_ft(row_text)
            year = parse_year_from_name(raw_name)
            name = clean_name(_YEAR_IN_NAME.sub("", raw_name).strip())

            key = normalize_name(name)
            if key in seen:
                continue
            seen.add(key)

            entry = {
                "name": name,
                "lat": lat,
                "lon": lon,
                "depth_ft": depth,
                "year_lost": year,
                "vessel_type": None,
                "lake": infer_lake(lat, lon),
                "preserve": preserve_name,
                "state": "Michigan",
                "source": url,
                "source_site": "michiganpreserves.org",
                "coord_quality": "preserve_registry",
            }
            results.append(entry)
            print(f"    {name:<36} {lat:.4f}, {lon:.4f}  depth={depth}")

    return results


def scrape_wikipedia_preserve(url: str) -> list[dict]:
    print(f"\n[Wikipedia] {url}")
    html = polite_get(url)
    if not html:
        return []

    soup = BeautifulSoup(html, "html.parser")
    preserve_name = soup.find("h1").get_text(strip=True) if soup.find("h1") else "Unknown"
    results: list[dict] = []
    seen: set[str] = set()

    for table in soup.find_all("table"):
        rows = table.find_all("tr")
        if len(rows) < 2:
            continue
        table_text = table.get_text()
        if "°" not in table_text and parse_coords(table_text) is None:
            continue

        for row in rows[1:]:
            cells = row.find_all(["td", "th"])
            if not cells:
                continue
            name = cells[0].get_text(strip=True)
            if not name or name.lower() in ("wreck name", "site name", "name", "vessel"):
                continue
            row_text = " ".join(c.get_text(" ", strip=True) for c in cells)
            coords = parse_coords(row_text)
            if not coords:
                continue

            lat, lon = coords
            depth = parse_depth_ft(row_text)
            vessel_type = None
            for c in cells[1:]:
                ct = c.get_text(strip=True)
                if parse_depth_ft(ct):
                    continue
                if not re.search(r"\d", ct) and 3 < len(ct) < 60 and vessel_type is None:
                    vessel_type = ct

            key = normalize_name(name)
            if key in seen:
                continue
            seen.add(key)

            results.append(
                {
                    "name": clean_name(name),
                    "lat": lat,
                    "lon": lon,
                    "depth_ft": depth,
                    "year_lost": parse_year_from_name(name),
                    "vessel_type": vessel_type,
                    "lake": infer_lake(lat, lon),
                    "preserve": preserve_name,
                    "state": "Michigan",
                    "source": url,
                    "source_site": "wikipedia.org",
                    "coord_quality": "preserve_registry",
                }
            )
    print(f"  → {len(results)} wrecks with GPS")
    return results


def scrape_wisconsin_sanctuary_wiki() -> list[dict]:
    print(f"\n[Wikipedia] Wisconsin Shipwreck Coast NMS")
    html = polite_get(WIKI_WI_SANCTUARY)
    if not html:
        return []

    soup = BeautifulSoup(html, "html.parser")
    results: list[dict] = []
    seen: set[str] = set()

    for table in soup.find_all("table", class_="wikitable"):
        rows = table.find_all("tr")
        if len(rows) < 3:
            continue
        for row in rows[1:]:
            cells = row.find_all(["td", "th"])
            if len(cells) < 4:
                continue
            name = cells[0].get_text(" ", strip=True)
            if not name or name.lower() in ("ship", "vessel", "name"):
                continue

            row_text = row.get_text(" ", strip=True)
            coords = parse_coords(row_text)
            if not coords:
                geo = row.find("span", class_="geo")
                if geo:
                    coords = parse_coords(geo.get_text())
            if not coords:
                continue

            lat, lon = coords
            vessel_type = cells[1].get_text(strip=True) if len(cells) > 1 else None
            year_text = cells[2].get_text(strip=True) if len(cells) > 2 else ""
            year_m = re.search(r"(\d{4})", year_text)
            depth = parse_depth_ft(row_text)

            key = normalize_name(name)
            if key in seen:
                continue
            seen.add(key)

            results.append(
                {
                    "name": clean_name(name),
                    "lat": lat,
                    "lon": lon,
                    "depth_ft": depth,
                    "year_lost": int(year_m.group(1)) if year_m else None,
                    "vessel_type": vessel_type or None,
                    "lake": "Lake Michigan",
                    "preserve": "Wisconsin Shipwreck Coast National Marine Sanctuary",
                    "state": "Wisconsin",
                    "source": WIKI_WI_SANCTUARY,
                    "source_site": "wikipedia.org",
                    "coord_quality": "sanctuary_registry",
                }
            )
    print(f"  → {len(results)} wrecks with GPS")
    return results


def scrape_thunderbay_index() -> list[str]:
    print(f"\n[Thunder Bay NOAA] index")
    html = polite_get(THUNDERBAY_INDEX)
    if not html:
        return []
    soup = BeautifulSoup(html, "html.parser")
    urls: list[str] = []
    for a in soup.find_all("a", href=True):
        href = a["href"]
        if "/shipwrecks/" in href and href.endswith(".html"):
            full = urljoin(THUNDERBAY_BASE, href)
            if full not in urls:
                urls.append(full)
    print(f"  {len(urls)} wreck pages")
    return urls


def scrape_thunderbay_wreck(url: str) -> dict | None:
    html = polite_get(url)
    if not html:
        return None
    soup = BeautifulSoup(html, "html.parser")
    h1 = soup.find("h1")
    name = h1.get_text(strip=True) if h1 else Path(urlparse(url).path).stem
    body_text = soup.get_text(" ", strip=True)
    coords = parse_coords(body_text)
    if coords is None:
        gps_line = re.search(
            r"GPS\s+Location[:\s]+([0-9°′'.NSEW\s,]+?)(?:Depth|Wreck Length|\n|$)",
            body_text,
            re.IGNORECASE,
        )
        if gps_line:
            coords = parse_coords(gps_line.group(1))

    depth_match = re.search(
        r"Depth[:\s]+(\d+)\s*(?:to\s*(\d+))?\s*feet", body_text, re.IGNORECASE
    )
    depth_ft = int(depth_match.group(1)) if depth_match else None
    type_match = re.search(r"Vessel\s+Type[:\s]+([^\n.]{3,50})", body_text, re.IGNORECASE)
    year_match = re.search(r"Wrecked[:\s]+.*?(\d{4})", body_text, re.IGNORECASE)

    return {
        "name": clean_name(name),
        "lat": coords[0] if coords else None,
        "lon": coords[1] if coords else None,
        "depth_ft": depth_ft,
        "year_lost": int(year_match.group(1)) if year_match else None,
        "vessel_type": type_match.group(1).strip() if type_match else None,
        "lake": "Lake Huron",
        "preserve": "Thunder Bay National Marine Sanctuary",
        "state": "Michigan",
        "source": url,
        "source_site": "thunderbay.noaa.gov",
        "coord_quality": "preserve_registry" if coords else "name_only",
    }


def deduplicate(records: list[dict]) -> list[dict]:
    by_name: dict[str, dict] = {}
    site_rank = {
        "michiganpreserves.org": 4,
        "thunderbay.noaa.gov": 3,
        "wikipedia.org": 2,
    }

    for r in records:
        key = normalize_name(r["name"])
        if not key:
            continue
        if key not in by_name:
            by_name[key] = r
            continue
        existing = by_name[key]
        if existing.get("lat") is None and r.get("lat") is not None:
            by_name[key] = r
            continue
        if r.get("lat") is None:
            continue
        er = site_rank.get(existing.get("source_site", ""), 0)
        rr = site_rank.get(r.get("source_site", ""), 0)
        if rr > er:
            by_name[key] = r
    return list(by_name.values())


def write_summary(records: list[dict], path: Path) -> None:
    with_gps = [r for r in records if r.get("lat") is not None]
    lines = [
        "Great Lakes preserve / sanctuary wreck scrape",
        f"Total unique wrecks: {len(records)}",
        f"With GPS: {len(with_gps)}",
        "",
        "By preserve:",
    ]
    by_preserve: dict[str, int] = {}
    for w in with_gps:
        p = w.get("preserve", "Unknown")
        by_preserve[p] = by_preserve.get(p, 0) + 1
    for p, n in sorted(by_preserve.items(), key=lambda x: (-x[1], x[0])):
        lines.append(f"  {n:>4}  {p}")
    lines.extend(["", "By source site:"])
    by_site: dict[str, int] = {}
    for w in records:
        s = w.get("source_site", "?")
        by_site[s] = by_site.get(s, 0) + 1
    for s, n in sorted(by_site.items(), key=lambda x: -x[1]):
        lines.append(f"  {n:>4}  {s}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--fast",
        action="store_true",
        help="Skip NOAA Thunder Bay per-wreck pages (index/wiki/michiganpreserves only)",
    )
    parser.add_argument("--delay", type=float, default=DELAY, help="Seconds between requests")
    args = parser.parse_args()
    global _request_delay
    _request_delay = args.delay

    all_wrecks: list[dict] = []

    print("=" * 60)
    print("PHASE 1: michiganpreserves.org (hub + subsites)")
    print("=" * 60)
    preserve_urls = discover_michigan_preserves()
    print(f"Discovered {len(preserve_urls)} preserve pages")
    for url in preserve_urls:
        all_wrecks.extend(scrape_michiganpreserves_page(url))

    print("\n" + "=" * 60)
    print("PHASE 2: Wikipedia Michigan preserves")
    print("=" * 60)
    for url in WIKI_MI_PRESERVES:
        all_wrecks.extend(scrape_wikipedia_preserve(url))

    print("\n" + "=" * 60)
    print("PHASE 3: Wisconsin Shipwreck Coast (Wikipedia)")
    print("=" * 60)
    all_wrecks.extend(scrape_wisconsin_sanctuary_wiki())

    if not args.fast:
        print("\n" + "=" * 60)
        print("PHASE 4: Thunder Bay NOAA individual pages")
        print("=" * 60)
        for url in scrape_thunderbay_index():
            entry = scrape_thunderbay_wreck(url)
            if entry:
                all_wrecks.append(entry)
                status = (
                    f"{entry['lat']:.4f},{entry['lon']:.4f}"
                    if entry["lat"]
                    else "no GPS"
                )
                print(f"  {entry['name']:<40} {status}")

    print(f"\nRaw records: {len(all_wrecks)}")
    all_wrecks = deduplicate(all_wrecks)
    with_gps = [w for w in all_wrecks if w.get("lat") is not None]
    print(f"Unique: {len(all_wrecks)}  |  With GPS: {len(with_gps)}")

    OUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    OUT_JSON.write_text(json.dumps(all_wrecks, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"Wrote {OUT_JSON}")

    fields = [
        "name",
        "lat",
        "lon",
        "depth_ft",
        "year_lost",
        "vessel_type",
        "lake",
        "state",
        "preserve",
        "coord_quality",
        "source_site",
        "source",
    ]
    with open(OUT_CSV, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fields, extrasaction="ignore")
        writer.writeheader()
        writer.writerows(all_wrecks)
    print(f"Wrote {OUT_CSV}")

    write_summary(all_wrecks, OUT_SUMMARY)
    print(f"Wrote {OUT_SUMMARY}")


if __name__ == "__main__":
    main()
