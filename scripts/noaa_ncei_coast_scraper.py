#!/usr/bin/env python3
"""
NOAA NCEI / NGDC coast index scraper + downloader.

Crawls directory listings under:
  https://data.ngdc.noaa.gov/platforms/ocean/nos/coast/

Writes:
  - metadata JSONL
  - file manifest TSV
Optionally downloads matching files (e.g., .bag).

Usage:
  # Metadata only (fast inventory)
  python3 scripts/noaa_ncei_coast_scraper.py --metadata-only

  # Download all BAG files under H* survey folders
  python3 scripts/noaa_ncei_coast_scraper.py --include-prefix H --download-ext .bag

  # Resume-safe bulk pull with custom output root
  python3 scripts/noaa_ncei_coast_scraper.py \
    --include-prefix H \
    --download-ext .bag \
    --output-root /data/cesarops/bathymetry/ncei_coast
"""

from __future__ import annotations

import argparse
import csv
import html.parser
import json
import os
import re
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, List
from urllib.parse import urljoin, urlparse

import requests

BASE_URL = "https://data.ngdc.noaa.gov/platforms/ocean/nos/coast/"
DEFAULT_OUT = Path("/data/cesarops/bathymetry/ncei_coast")
UA = "cesarops-ncei-scraper/1.0"

# Named product presets. Encodes the team decision about which per-survey
# product folders + file types we actually want, so nightly runs use a single
# stable flag instead of a fragile multi-flag command line.
#
# Survey vintage changes the backscatter folder name:
#   * older surveys (H12001-era): TIFF/  (side-scan, SSSAB)
#   * newer surveys (H13252+):    MBAB/  (multibeam acoustic backscatter)
# so the "detect" preset includes BOTH and lets subdir filtering pick whichever
# a given survey actually has.
PRODUCT_PRESETS: dict[str, dict[str, list[str]]] = {
    # Everything useful to the wreck/redaction detectors + the stitch-seam work.
    "detect": {
        "subdirs": ["BAG", "TIFF", "MBAB", "DR", "GEODAS", "Bottom_Samples"],
        "exts": [".bag", ".tif", ".tiff", ".pdf", ".xml", ".gz", ".ascii", ".zip"],
    },
    # Bathymetry only (smallest footprint).
    "bag": {
        "subdirs": ["BAG"],
        "exts": [".bag"],
    },
    # Bathy + backscatter image (fast first-pass pair), no metadata.
    "imagery": {
        "subdirs": ["BAG", "TIFF", "MBAB"],
        "exts": [".bag", ".tif", ".tiff"],
    },
}

# Mission support:
# - "greatlakes": use each survey report's Locality field (e.g. "Lake Erie")
#   to filter and save outputs under .../files/<lake>/...
GREAT_LAKES = {
    "Lake Superior": "lake_superior",
    "Lake Michigan": "lake_michigan",
    "Lake Huron": "lake_huron",
    "Lake Erie": "lake_erie",
    "Lake Ontario": "lake_ontario",
}

LOCALITY_RE = re.compile(
    r"<b>\s*Locality:\s*</b>\s*</td>\s*<td[^>]*>\s*([^<]+?)\s*</td>",
    re.IGNORECASE | re.DOTALL,
)


@dataclass
class IndexItem:
    href: str
    name: str
    is_dir: bool


class IndexParser(html.parser.HTMLParser):
    """Parse simple Apache/nginx index anchor list."""

    def __init__(self) -> None:
        super().__init__()
        self._in_a = False
        self._href = ""
        self._text = ""
        self.items: List[IndexItem] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag.lower() != "a":
            return
        self._in_a = True
        self._text = ""
        self._href = ""
        for k, v in attrs:
            if k.lower() == "href" and v:
                self._href = v

    def handle_data(self, data: str) -> None:
        if self._in_a:
            self._text += data

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() != "a" or not self._in_a:
            return
        self._in_a = False
        name = self._text.strip()
        href = self._href.strip()
        if not href or not name or name.lower() == "parent directory":
            return
        is_dir = href.endswith("/")
        self.items.append(IndexItem(href=href, name=name.rstrip("/"), is_dir=is_dir))


def fetch_text(url: str, timeout: int = 25, retries: int = 5) -> str:
    headers = {"User-Agent": UA}
    backoff = 2.0
    last_err: Exception | None = None
    for _ in range(retries):
        try:
            r = requests.get(url, headers=headers, timeout=timeout)
            r.raise_for_status()
            return r.text
        except Exception as e:  # noqa: BLE001
            last_err = e
            time.sleep(backoff)
            backoff = min(backoff * 1.7, 20.0)
    raise RuntimeError(f"failed GET {url}: {last_err}")


def parse_index(url: str) -> list[IndexItem]:
    html = fetch_text(url)
    p = IndexParser()
    p.feed(html)
    return p.items


def rel_from_base(url: str, base_url: str) -> str:
    if not url.startswith(base_url):
        return url
    rel = url[len(base_url) :]
    return rel.lstrip("/")


def ext_ok(name: str, allowed: set[str]) -> bool:
    if not allowed:
        return True
    lower = name.lower()
    return any(lower.endswith(ext) for ext in allowed)


def prefix_ok(rel: str, prefixes: list[str]) -> bool:
    if not prefixes:
        return True
    top = rel.split("/", 1)[0]
    return any(top.upper().startswith(p.upper()) for p in prefixes)


def subdir_ok(rel: str, subdirs: set[str]) -> bool:
    """Per-survey product-folder filter.

    NCEI survey layout is ``<range>/<survey>/<subdir>/<file>`` e.g.
    ``H12001-H14000/H12001/BAG/H12001_MB_1m_MLLW_1of2.bag``. When ``subdirs`` is
    non-empty we only descend into / keep paths whose 3rd component (the product
    folder: BAG, TIFF, DR, GEODAS, ...) is allowed. Paths shallower than the
    subdir level (range or survey directories) always pass so the crawler can
    still reach the product folders. This lets us skip TIDES/project_sketches.
    """
    if not subdirs:
        return True
    parts = [p for p in rel.strip("/").split("/") if p]
    if len(parts) < 3:
        return True  # range or survey level — keep descending
    return parts[2].upper() in subdirs


def crawl(
    base_url: str,
    roots: Iterable[str],
    include_prefix: list[str],
    include_ext: set[str],
    metadata_jsonl: Path,
    manifest_tsv: Path,
    include_subdir: set[str] | None = None,
) -> list[dict]:
    include_subdir = include_subdir or set()
    stack = [urljoin(base_url, r) for r in roots]
    seen = set()
    files: list[dict] = []

    metadata_jsonl.parent.mkdir(parents=True, exist_ok=True)
    with metadata_jsonl.open("w", encoding="utf-8") as mj:
        while stack:
            url = stack.pop()
            if url in seen:
                continue
            seen.add(url)

            try:
                items = parse_index(url)
            except Exception as e:  # noqa: BLE001
                rec = {"type": "error", "url": url, "error": str(e), "ts": int(time.time())}
                mj.write(json.dumps(rec) + "\n")
                continue

            rec = {
                "type": "index",
                "url": url,
                "rel": rel_from_base(url, base_url),
                "entries": len(items),
                "ts": int(time.time()),
            }
            mj.write(json.dumps(rec) + "\n")

            for it in items:
                child = urljoin(url, it.href)
                rel = rel_from_base(child, base_url)
                if it.is_dir:
                    if prefix_ok(rel, include_prefix) and subdir_ok(rel, include_subdir):
                        stack.append(child)
                    continue
                if not prefix_ok(rel, include_prefix):
                    continue
                if not subdir_ok(rel, include_subdir):
                    continue
                if not ext_ok(it.name, include_ext):
                    continue
                files.append(
                    {
                        "name": it.name,
                        "url": child,
                        "rel": rel,
                    }
                )

    # Write compact manifest TSV for bulk ops.
    manifest_tsv.parent.mkdir(parents=True, exist_ok=True)
    with manifest_tsv.open("w", encoding="utf-8", newline="") as f:
        w = csv.writer(f, delimiter="\t")
        w.writerow(["rel", "url", "name"])
        for row in files:
            w.writerow([row["rel"], row["url"], row["name"]])

    return files


def file_size(url: str, timeout: int = 20) -> int | None:
    try:
        r = requests.head(url, headers={"User-Agent": UA}, timeout=timeout, allow_redirects=True)
        if "Content-Length" in r.headers:
            return int(r.headers["Content-Length"])
    except Exception:  # noqa: BLE001
        return None
    return None


def download_one(url: str, dst: Path, retries: int = 4) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    tmp = dst.with_suffix(dst.suffix + ".part")
    existing = tmp.stat().st_size if tmp.exists() else 0
    headers = {"User-Agent": UA}
    if existing > 0:
        headers["Range"] = f"bytes={existing}-"

    backoff = 2.0
    last_err: Exception | None = None
    for _ in range(retries):
        try:
            with requests.get(url, headers=headers, timeout=90, stream=True) as r:
                if r.status_code not in (200, 206):
                    r.raise_for_status()
                mode = "ab" if r.status_code == 206 and existing > 0 else "wb"
                with tmp.open(mode) as out:
                    for chunk in r.iter_content(chunk_size=4 * 1024 * 1024):
                        if chunk:
                            out.write(chunk)
            tmp.replace(dst)
            return
        except Exception as e:  # noqa: BLE001
            last_err = e
            time.sleep(backoff)
            backoff = min(backoff * 1.8, 25.0)
    raise RuntimeError(f"download failed {url}: {last_err}")


def main() -> int:
    ap = argparse.ArgumentParser(description="Scrape/download NOAA NCEI coast index files")
    ap.add_argument("--base-url", default=BASE_URL)
    ap.add_argument(
        "--roots",
        default="H00001-H02000/,H02001-H04000/,H04001-H06000/,H06001-H08000/,H08001-H10000/,H10001-H12000/,H12001-H14000/,H14001-H16000/",
        help="Comma-separated index roots under base URL",
    )
    ap.add_argument(
        "--include-prefix",
        action="append",
        default=[],
        help="Top-level prefix filter (repeatable). Example: --include-prefix H",
    )
    ap.add_argument(
        "--download-ext",
        action="append",
        default=[".bag"],
        help="File extension to keep/download (repeatable). Example: .bag .xml",
    )
    ap.add_argument(
        "--include-subdir",
        action="append",
        default=[],
        help="Per-survey product folder to keep (repeatable, case-insensitive). "
        "Example: --include-subdir BAG --include-subdir TIFF --include-subdir DR "
        "--include-subdir GEODAS. Skips TIDES/project_sketches when set.",
    )
    ap.add_argument(
        "--products",
        choices=sorted(PRODUCT_PRESETS.keys()),
        default=None,
        help="Named product preset selecting subdirs + extensions. "
        "'detect' = BAG+backscatter(TIFF/MBAB)+DR(pdf/xml)+GEODAS+Bottom_Samples; "
        "'bag' = bathymetry only; 'imagery' = bathy+backscatter. "
        "Explicit --download-ext / --include-subdir override the preset.",
    )
    ap.add_argument("--metadata-only", action="store_true", help="Crawl index + write metadata only")
    ap.add_argument("--output-root", default=str(DEFAULT_OUT))
    ap.add_argument(
        "--mission",
        default="general",
        choices=["general", "greatlakes"],
        help="How to interpret survey reports during download",
    )
    ap.add_argument("--limit", type=int, default=0, help="Max files to download (0 = all)")
    args = ap.parse_args()

    base_url = args.base_url if args.base_url.endswith("/") else args.base_url + "/"
    roots = [r.strip() for r in args.roots.split(",") if r.strip()]
    include_prefix = [p.strip() for p in args.include_prefix if p.strip()]

    # Apply a named product preset unless the user explicitly set ext/subdir.
    # argparse default for --download-ext is [".bag"]; treat that exact default
    # as "unset" so a preset can fill it in.
    ext_overridden = args.download_ext != [".bag"]
    subdir_overridden = bool(args.include_subdir)
    if args.products:
        preset = PRODUCT_PRESETS[args.products]
        if not ext_overridden:
            args.download_ext = list(preset["exts"])
        if not subdir_overridden:
            args.include_subdir = list(preset["subdirs"])

    include_ext = {e.lower() if e.startswith(".") else f".{e.lower()}" for e in args.download_ext}
    include_subdir = {s.strip().upper() for s in args.include_subdir if s.strip()}

    out = Path(args.output_root)
    out.mkdir(parents=True, exist_ok=True)
    ts = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    metadata_jsonl = out / f"index_metadata_{ts}.jsonl"
    manifest_tsv = out / f"manifest_{ts}.tsv"
    summary_json = out / f"summary_{ts}.json"
    dl_root = out / "files"

    # Map survey ID (e.g. H13607) to its Hxxxx-Hyyyy range so we can build the survey report URL.
    root_ranges: list[tuple[int, int, str]] = []
    for r in roots:
        clean = r.rstrip("/")
        m = re.match(r"^H(\d+)-H(\d+)$", clean, flags=re.IGNORECASE)
        if not m:
            continue
        lo = int(m.group(1))
        hi = int(m.group(2))
        root_ranges.append((lo, hi, clean))
    if not root_ranges:
        # Fall back to empty (greatlakes mission may still work by skipping if range is unknown).
        print("[warn] could not parse any Hxxxx-Hyyyy roots")

    def range_for_survey_id(survey_id: str) -> str | None:
        m = re.match(r"^H(\d+)$", survey_id, flags=re.IGNORECASE)
        if not m:
            return None
        n = int(m.group(1))
        for lo, hi, root_range in root_ranges:
            if lo <= n <= hi:
                return root_range
        return None

    def classify_great_lakes(survey_id: str) -> str | None:
        """
        Returns canonical lake folder name (e.g. lake_erie) or None if not a Great Lake.
        """
        root_range = range_for_survey_id(survey_id)
        if not root_range:
            return None

        # Example (from NOAA pages):
        # https://www.ngdc.noaa.gov/nos/H12001-H14000/H13607.html
        report_url = f"https://www.ngdc.noaa.gov/nos/{root_range}/{survey_id}.html"
        html_text = fetch_text(report_url, timeout=30, retries=4)
        m = LOCALITY_RE.search(html_text)
        if not m:
            return None
        locality = m.group(1).strip()
        for k, v in GREAT_LAKES.items():
            if locality.lower() == k.lower():
                return v
        return None

    files = crawl(
        base_url=base_url,
        roots=roots,
        include_prefix=include_prefix,
        include_ext=include_ext,
        metadata_jsonl=metadata_jsonl,
        manifest_tsv=manifest_tsv,
        include_subdir=include_subdir,
    )
    print(f"[crawl] matched files={len(files)} metadata={metadata_jsonl} manifest={manifest_tsv}")

    summary = {
        "base_url": base_url,
        "roots": roots,
        "include_prefix": include_prefix,
        "download_ext": sorted(include_ext),
        "include_subdir": sorted(include_subdir),
        "products_preset": args.products,
        "matched_files": len(files),
        "downloaded_files": 0,
        "downloaded_bytes": 0,
        "skipped_files": 0,
        "skipped_bytes": 0,
        "ts": ts,
    }

    if args.metadata_only:
        summary_json.write_text(json.dumps(summary, indent=2), encoding="utf-8")
        print(f"[done] metadata-only summary={summary_json}")
        return 0

    lake_cache: dict[str, str | None] = {}
    downloaded_by_lake: dict[str, int] = {}

    to_dl = files[: args.limit] if args.limit and args.limit > 0 else files
    for i, item in enumerate(to_dl, start=1):
        rel = item["rel"]
        url = item["url"]
        survey_id = rel.split("/", 1)[0] if "/" in rel else ""

        dst = dl_root / rel
        if args.mission == "greatlakes" and survey_id:
            if survey_id not in lake_cache:
                lake_cache[survey_id] = classify_great_lakes(survey_id)
            lake = lake_cache[survey_id]
            if not lake:
                print(f"[skip {i}/{len(to_dl)}] {rel} (not a Great Lake)")
                continue
            dst = dl_root / lake / rel
            downloaded_by_lake[lake] = downloaded_by_lake.get(lake, 0) + 1

        sz = file_size(url)

        # If the destination file already exists and matches the server's
        # declared content length, treat it as "identical" and skip.
        if dst.exists() and sz is not None:
            try:
                if dst.stat().st_size == sz:
                    print(f"[skip {i}/{len(to_dl)}] {rel} (size match {sz} bytes)")
                    summary["skipped_files"] += 1
                    summary["skipped_bytes"] += sz
                    continue
            except OSError:
                # If stat fails for any reason, fall back to the normal download flow.
                pass

        print(f"[dl {i}/{len(to_dl)}] {rel} -> {dst}")
        download_one(url, dst)
        summary["downloaded_files"] += 1
        if sz:
            summary["downloaded_bytes"] += sz
        else:
            try:
                summary["downloaded_bytes"] += dst.stat().st_size
            except FileNotFoundError:
                pass

    summary_json.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    if args.mission == "greatlakes":
        # Persist download distribution for later auditing.
        summary_json.write_text(
            json.dumps({**json.loads(summary_json.read_text(encoding="utf-8")), "downloaded_by_lake": downloaded_by_lake}, indent=2),
            encoding="utf-8",
        )
    print(f"[done] downloaded={summary['downloaded_files']} bytes={summary['downloaded_bytes']} summary={summary_json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

