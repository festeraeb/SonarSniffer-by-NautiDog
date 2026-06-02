#!/usr/bin/env python3
"""Probe FEDEO discovery endpoints using FEDEO_* environment variables.

Usage:
  python3 scripts/fedeo_probe.py
  FEDEO_BASE_URL=https://fedeo.ceos.org python3 scripts/fedeo_probe.py --json
"""

from __future__ import annotations

import argparse
import json
import os
from dataclasses import asdict, dataclass
from typing import Optional

import requests


@dataclass
class ProbeResult:
    name: str
    url: str
    ok: bool
    status: int
    content_type: str
    note: str


def _env(key: str, default: str) -> str:
    val = os.environ.get(key, "").strip()
    return val if val else default


def probe(name: str, url: str, timeout: int, auth: Optional[tuple[str, str]]) -> ProbeResult:
    try:
        resp = requests.get(url, timeout=timeout, allow_redirects=True, auth=auth)
        ctype = resp.headers.get("content-type", "")
        ok = 200 <= resp.status_code < 300
        note = ""

        if "json" in ctype.lower():
            try:
                data = resp.json()
                if isinstance(data, dict):
                    note = f"keys={len(data.keys())}"
                elif isinstance(data, list):
                    note = f"items={len(data)}"
            except Exception:
                note = "json_parse_failed"
        elif "xml" in ctype.lower() or "html" in ctype.lower():
            text = (resp.text or "").strip()
            note = f"body_len={len(text)}"
        else:
            note = "response_received"

        return ProbeResult(
            name=name,
            url=url,
            ok=ok,
            status=resp.status_code,
            content_type=ctype,
            note=note,
        )
    except Exception as exc:
        return ProbeResult(
            name=name,
            url=url,
            ok=False,
            status=0,
            content_type="",
            note=f"error={type(exc).__name__}",
        )


def main() -> int:
    parser = argparse.ArgumentParser(description="Probe FEDEO API/OpenSearch/STAC endpoints")
    parser.add_argument("--timeout", type=int, default=30, help="HTTP timeout in seconds")
    parser.add_argument("--json", action="store_true", help="Print machine-readable JSON output")
    args = parser.parse_args()

    base = _env("FEDEO_BASE_URL", "https://fedeo.ceos.org").rstrip("/")
    api = _env("FEDEO_API_URL", f"{base}/api")
    stac = _env("FEDEO_STAC_URL", f"{base}/")
    desc = _env(
        "FEDEO_OPENSEARCH_DESCRIPTION_URL",
        f"{api}?httpAccept=application%2Fopensearchdescription%2Bxml",
    )
    explain = _env(
        "FEDEO_EXPLAIN_URL",
        f"{api}?httpAccept=application%2Fjson%3Bprofile%3D%22http%3A%2F%2Fexplain.z3950.org%2Fdtd%2F2.0%2F%22",
    )

    # Simple collection discovery request from the FEDEO readme examples.
    series_sample = (
        f"{base}/collections/series/items?startRecord=1&limit=3"
        "&organisationName=ESA/ESRIN&query=Forestry&httpAccept=application%2Fatom%2Bxml"
    )

    user = os.environ.get("FEDEO_USERNAME", "").strip()
    password = os.environ.get("FEDEO_PASSWORD", "").strip()
    auth = (user, password) if user and password else None

    targets = [
        ("api_root", api),
        ("stac_root", stac),
        ("opensearch_description", desc),
        ("explain", explain),
        ("series_sample", series_sample),
    ]

    results = [probe(name, url, args.timeout, auth) for name, url in targets]

    if args.json:
        print(json.dumps([asdict(r) for r in results], indent=2))
    else:
        print("FEDEO Probe Results")
        print("=" * 72)
        for r in results:
            status = "OK" if r.ok else "FAIL"
            print(f"{r.name:24s} {status:5s} http={r.status:<3d} ctype={r.content_type}")
            print(f"  {r.url}")
            if r.note:
                print(f"  note: {r.note}")
        print("=" * 72)

    return 0 if all(r.ok for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
