#!/usr/bin/env python3
"""Download highest-resolution Great Lakes aeromagnetic + towed-mag datasets.

Direct HTTP where possible; NRCan GDR portal via Playwright fallback.

Output: /data/cesarops/aeromag/raw/{usgs,lake_superior,nrcan,towed}/
Manifest: /data/cesarops/aeromag/meta/great_lakes_fetch_manifest.json
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import time
import zipfile
from datetime import datetime, timezone
from pathlib import Path

import requests

REPO = Path(__file__).resolve().parents[1]
AEROMAG_ROOT = Path(__import__("os").environ.get("AEROMAG_ROOT", "/data/cesarops/aeromag"))
RAW = AEROMAG_ROOT / "raw"
META = AEROMAG_ROOT / "meta"
UA = {"User-Agent": "CESAROPS-AeromagGreatLakes/1.0 (wreck-detection research)"}

# ── Direct-download catalog ───────────────────────────────────────────────────

USGS_MRDATA = [
    ("NAmag_hp500.zip", "USGS NAmag high-pass 500km — local anomalies (~317 MB)", 1),
    ("NAmag_origmrg.zip", "USGS NAmag original merged (~315 MB)", 1),
    ("USmag_hp500.zip", "USGS USmag high-pass 500km (~47 MB)", 2),
    ("USmag_origmrg.zip", "USGS USmag original merged (~46 MB)", 2),
    ("magnetic.xyz.gz", "US contiguous US XYZ text NAD27 (~23 MB)", 3),
    ("magnetic.gxf.gz", "US GXF grid Albers NAD27 (~11 MB)", 3),
]

# USGS ScienceBase — US+Canada merged GeoTIFF (DOI 10.5066/P970GDD5)
SCIENCEBASE_USCANADA = "619a9a3ad34eb622f692f961"
USGS_SCIENCEBASE_MAG = [
    (
        f"https://www.sciencebase.gov/catalog/file/get/{SCIENCEBASE_USCANADA}?name=GeophysicsMag_USCanada.zip",
        "usgs/GeophysicsMag_USCanada.zip",
        "US+Canada residual magnetic anomaly GeoTIFF (~127 MB)",
    ),
    (
        f"https://www.sciencebase.gov/catalog/file/get/{SCIENCEBASE_USCANADA}?name=GeophysicsMag_RTP_USCanada.zip",
        "usgs/GeophysicsMag_RTP_USCanada.zip",
        "US+Canada RTP magnetic anomaly GeoTIFF (~128 MB)",
    ),
    (
        f"https://www.sciencebase.gov/catalog/file/get/{SCIENCEBASE_USCANADA}?name=GeophysicsMag_RTP_VD_USCanada.zip",
        "usgs/GeophysicsMag_RTP_VD_USCanada.zip",
        "US+Canada 1st vertical derivative RTP mag (~129 MB)",
    ),
    (
        f"https://www.sciencebase.gov/catalog/file/get/{SCIENCEBASE_USCANADA}?name=GeophysicsMagRTP_DeepSources_USCanada.zip",
        "usgs/GeophysicsMagRTP_DeepSources_USCanada.zip",
        "US+Canada long-wavelength RTP mag (~29 MB)",
    ),
]

LAKE_SUPERIOR = [
    (
        "https://www.sciencebase.gov/catalog/file/get/594993a8e4b062508e359ba7?name=LakeSupRegMag300.tif",
        "lake_superior/LakeSupRegMag300.tif",
        "Lake Superior aeromag 300m observation height (deep water, 500m cells)",
    ),
    (
        "https://www.sciencebase.gov/catalog/file/get/594993a8e4b062508e359ba7?name=LakeSupRegMag150.tif",
        "lake_superior/LakeSupRegMag150.tif",
        "Lake Superior aeromag 150m observation height (nearshore, 250m cells)",
    ),
    (
        "https://www.sciencebase.gov/catalog/file/get/594993a8e4b062508e359ba7?name=LakeSupRegMag300.gxf",
        "lake_superior/LakeSupRegMag300.gxf",
        "Lake Superior mag 300m GXF",
    ),
]

NRCAN_GDR_DAPIDS = {
    "nrcan_ca_200m_rtf": {"dapid": "140", "title": "Canada 200m MAG Residual Total Field"},
    "nrcan_ca_200m_vd": {"dapid": "138", "title": "Canada 200m MAG 1st Vertical Derivative"},
    "nrcan_ca_1km_rtf": {"dapid": "129", "title": "Canada 1km MAG Residual Total Field"},
    "cagdb_erie_lake": {"db_project_no": "122", "title": "CAGDB Erie Lake flight-line survey"},
    "cagdb_huron_lake": {"db_project_no": "176", "title": "CAGDB Huron Lake flight-line survey"},
}

ONTARIO_CATALOG = [
    (
        "https://www.geologyontario.mndm.gov.on.ca/mines/data/google/web_kml/magnetics.kml",
        "ontario/ontario_master_magnetics.kml",
        "Ontario single-master aeromag KML (Ontario Data Catalogue)",
    ),
    (
        "https://data.ontario.ca/dataset/geophysical-dataset-index/resource/"
        "recent/download/geophysical_dataset_index_en.kml",
        "ontario/geophysical_dataset_index.kml",
        "Ontario geophysical survey index KML (links to downloadable OGS grids)",
    ),
]

NCEI_TOWED_CATALOG = [
    {
        "id": "noaa_21greatlakes_report",
        "url": "https://archive.oceanexplorer.noaa.gov/explorations/21greatlakes/21greatlakes-final-report.pdf",
        "dest": "towed/noaa_21greatlakes_final_report.pdf",
        "note": "Marine Magnetics towed + AUV mag; data archived at NCEI OER atlas",
    },
    {
        "id": "ncei_trackline_portal",
        "url": "https://www.ngdc.noaa.gov/trackline/request/?west=-92&east=-76&south=41.5&north=49&magnetic=on",
        "dest": "towed/ncei_trackline_great_lakes_request.url",
        "note": "NOAA marine trackline geophysical — use web form; saved deep-link with Great Lakes bbox",
        "min_bytes": 100,
    },
]


def download_file(url: str, dest: Path, desc: str = "", min_bytes: int = 10_000, timeout: tuple[int, int] | int = 900) -> dict:
    if dest.exists() and dest.stat().st_size >= min_bytes:
        return {"ok": True, "cached": True, "bytes": dest.stat().st_size, "path": str(dest)}

    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    try:
        with requests.get(url, headers=UA, stream=True, timeout=timeout, allow_redirects=True) as r:
            if r.status_code != 200:
                return {"ok": False, "error": f"HTTP {r.status_code}", "url": url}
            total = int(r.headers.get("content-length", 0))
            # Reject HTML splash pages masquerading as downloads
            ctype = (r.headers.get("content-type") or "").lower()
            if "text/html" in ctype and total < 50_000:
                return {"ok": False, "error": "HTML landing page (portal redirect)", "url": url}
            downloaded = 0
            t0 = time.time()
            with tmp.open("wb") as fh:
                for chunk in r.iter_content(1 << 20):
                    if chunk:
                        fh.write(chunk)
                        downloaded += len(chunk)
                        if total and downloaded % (10 << 20) < (1 << 20):
                            pct = 100 * downloaded / total
                            rate = downloaded / 1e6 / max(time.time() - t0, 0.1)
                            print(f"\r  {dest.name}: {pct:5.1f}% {downloaded/1e6:.0f}MB {rate:.1f}MB/s", end="", flush=True)
            print()
        if downloaded < min_bytes:
            tmp.unlink(missing_ok=True)
            return {"ok": False, "error": f"too small ({downloaded} bytes)", "url": url}
        if dest.suffix.lower() == ".zip" and not zipfile.is_zipfile(tmp):
            tmp.unlink(missing_ok=True)
            return {"ok": False, "error": "not a zip file", "url": url}
        tmp.replace(dest)
        print(f"  ✓ {desc or dest.name} ({downloaded/1e6:.1f} MB)")
        return {"ok": True, "bytes": downloaded, "path": str(dest), "url": url}
    except Exception as e:
        tmp.unlink(missing_ok=True)
        return {"ok": False, "error": str(e), "url": url}


def download_usgs(priority: int = 2) -> list[dict]:
    out = []
    d = RAW / "usgs"
    d.mkdir(parents=True, exist_ok=True)
    for fname, desc, pri in USGS_MRDATA:
        if pri > priority:
            continue
        url = f"https://mrdata.usgs.gov/magnetic/{fname}"
        rec = download_file(url, d / fname, desc, min_bytes=1_000_000)
        rec.update({"id": fname, "source": "usgs_mrdata"})
        out.append(rec)
    return out


def download_lake_superior() -> list[dict]:
    out = []
    for url, rel, desc in LAKE_SUPERIOR:
        rec = download_file(url, RAW / rel, desc, min_bytes=100_000)
        rec.update({"id": Path(rel).name, "source": "usgs_lake_superior_compilation"})
        out.append(rec)
    return out


def fetch_nrcan_via_playwright(dapids: list[str], out_dir: Path) -> list[dict]:
    """Use Playwright to navigate geophysical-data.canada.ca and capture download URLs."""
    results = []
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        pw_root = REPO / "scripts/forge_collab/node_modules/playwright"
        if pw_root.exists():
            sys.path.insert(0, str(pw_root.parent))
        try:
            from playwright.sync_api import sync_playwright
        except ImportError:
            return [{"ok": False, "error": "playwright not installed"}]

    portal_urls = [
        "https://geophysical-data.canada.ca/portal/",
        "https://geophysical-data.canada.ca/",
    ]
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page()
        for portal in portal_urls:
            try:
                page.goto(portal, wait_until="networkidle", timeout=120_000)
                break
            except Exception:
                continue

        # Search for aeromagnetic compilation
        for term in ["200m magnetic", "residual total field", "aeromagnetic compilation"]:
            try:
                search = page.locator('input[type="search"], input[name="q"], #search, .search-input').first
                if search.count():
                    search.fill(term)
                    search.press("Enter")
                    page.wait_for_timeout(3000)
                    break
            except Exception:
                pass

        links = page.eval_on_selector_all(
            "a[href]",
            "els => els.map(e => ({href: e.href, text: (e.innerText||'').trim()}))",
        )
        zip_links = [
            l for l in links
            if re.search(r"\.(zip|tif|gxf|grd)(\?|$)", l["href"], re.I)
            or "download" in l["href"].lower()
            or "dapid" in l["href"].lower()
        ]

        for dap in dapids:
            dest = out_dir / f"nrcan_dapid_{dap}.zip"
            if dest.exists() and dest.stat().st_size > 1_000_000:
                results.append({"ok": True, "cached": True, "dapid": dap, "path": str(dest)})
                continue
            matched = [l for l in zip_links if f"dapid={dap}" in l["href"] or dap in l["text"]]
            if not matched:
                # Try direct navigation to legacy URL and intercept download
                legacy = f"https://gdr.agg.nrcan.gc.ca/gdrdap/dap/index-eng.php?dapid={dap}"
                try:
                    with page.expect_download(timeout=60_000) as dl_info:
                        page.goto(legacy)
                    download = dl_info.value
                    download.save_as(dest)
                    results.append({"ok": True, "dapid": dap, "path": str(dest), "via": "expect_download"})
                    continue
                except Exception as e:
                    results.append({"ok": False, "dapid": dap, "error": str(e), "links_found": len(zip_links)})
                    continue
            url = matched[0]["href"]
            rec = download_file(url, dest, f"NRCan dapid={dap}")
            rec["dapid"] = dap
            results.append(rec)
        browser.close()
    return results


def download_nrcan_direct() -> list[dict]:
    """Try legacy download.php before Playwright."""
    out = []
    d = RAW / "nrcan"
    d.mkdir(parents=True, exist_ok=True)
    for key, spec in NRCAN_GDR_DAPIDS.items():
        if "db_project_no" in spec:
            url = f"https://gdr.agg.nrcan.gc.ca/gdrdap/dap/download.php?db_project_no={spec['db_project_no']}"
            dap = spec["db_project_no"]
        else:
            url = f"https://gdr.agg.nrcan.gc.ca/gdrdap/dap/download.php?dapid={spec['dapid']}"
            dap = spec["dapid"]
        dest = d / f"{key}.zip"
        rec = download_file(url, dest, spec["title"], min_bytes=500_000, timeout=(15, 120))
        rec.update({"id": key, "dapid": dap, "source": "nrcan_gdr"})
        out.append(rec)
    return out


def download_usgs_sciencebase() -> list[dict]:
    out = []
    for url, rel, desc in USGS_SCIENCEBASE_MAG:
        rec = download_file(url, RAW / rel, desc, min_bytes=1_000_000)
        rec.update({"id": Path(rel).name, "source": "usgs_sciencebase_uscanada", "doi": "10.5066/P970GDD5"})
        out.append(rec)
    return out


def download_ontario_catalog() -> list[dict]:
    out = []
    for url, rel, desc in ONTARIO_CATALOG:
        rec = download_file(url, RAW / rel, desc, min_bytes=1000, timeout=(15, 60))
        rec.update({"id": Path(rel).name, "source": "ontario_data_catalogue"})
        out.append(rec)
    return out


def download_towed_catalog() -> list[dict]:
    out = []
    for item in NCEI_TOWED_CATALOG:
        rec = download_file(item["url"], RAW / item["dest"], item["id"], min_bytes=item.get("min_bytes", 1000), timeout=(15, 120))
        rec.update({"id": item["id"], "note": item.get("note"), "source": "noaa_ncei_towed"})
        out.append(rec)
    return out


def fetch_ncei_oer_great_lakes() -> list[dict]:
    """Query NCEI OER metadata for 2021 Great Lakes magnetometer surveys."""
    out = []
    # NCEI ISO19115 search for Great Lakes OER magnetometer
    endpoints = [
        "https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ncei:EX2106",
        "https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ncei:EX2107",
    ]
    d = RAW / "towed" / "ncei_metadata"
    d.mkdir(parents=True, exist_ok=True)
    for url in endpoints:
        dest = d / (url.split(":")[-1] + ".xml")
        rec = download_file(url, dest, url.split(":")[-1], min_bytes=500, timeout=(15, 60))
        out.append(rec)
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--usgs-priority", type=int, default=3, help="1=NA only, 2=+US subset, 3=+xyz/gxf")
    ap.add_argument("--skip-sciencebase", action="store_true", help="Skip large US+Canada GeoTIFF zips")
    ap.add_argument("--skip-playwright", action="store_true")
    ap.add_argument("--playwright-only", action="store_true")
    args = ap.parse_args()

    RAW.mkdir(parents=True, exist_ok=True)
    META.mkdir(parents=True, exist_ok=True)

    manifest = {
        "fetched_at": datetime.now(timezone.utc).isoformat(),
        "root": str(AEROMAG_ROOT),
        "sources": {},
    }

    if not args.playwright_only:
        print("\n=== USGS mrdata ===")
        manifest["sources"]["usgs"] = download_usgs(args.usgs_priority)
        if not args.skip_sciencebase:
            print("\n=== USGS ScienceBase US+Canada GeoTIFF ===")
            manifest["sources"]["usgs_sciencebase"] = download_usgs_sciencebase()
        print("\n=== Lake Superior compilation ===")
        manifest["sources"]["lake_superior"] = download_lake_superior()
        print("\n=== NRCan GDR (direct) ===")
        manifest["sources"]["nrcan_direct"] = download_nrcan_direct()
        print("\n=== Ontario master grid (KML) ===")
        manifest["sources"]["ontario"] = download_ontario_catalog()
        print("\n=== Towed / marine mag catalog ===")
        manifest["sources"]["towed"] = download_towed_catalog()
        manifest["sources"]["ncei_oer_metadata"] = fetch_ncei_oer_great_lakes()

    if not args.skip_playwright:
        failed_daps = []
        for rec in manifest.get("sources", {}).get("nrcan_direct", []):
            if not rec.get("ok"):
                failed_daps.append(rec.get("dapid"))
        if args.playwright_only or failed_daps:
            print("\n=== NRCan GDR (Playwright portal) ===")
            daps = failed_daps or [
                spec.get("dapid") or spec.get("db_project_no")
                for spec in NRCAN_GDR_DAPIDS.values()
            ]
            manifest["sources"]["nrcan_playwright"] = fetch_nrcan_via_playwright(
                daps, RAW / "nrcan"
            )

    manifest_path = META / "great_lakes_fetch_manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2))
    ok = sum(
        1
        for group in manifest["sources"].values()
        for r in group
        if isinstance(r, dict) and r.get("ok")
    )
    total = sum(len(g) for g in manifest["sources"].values())
    print(f"\nManifest: {manifest_path}")
    print(f"OK: {ok}/{total}")
    return 0 if ok >= 3 else 1


if __name__ == "__main__":
    raise SystemExit(main())
