#!/usr/bin/env python3
"""
NCEI NOS Survey Scraper — Great Lakes BAG Files
Scrapes all survey ranges from https://www.ngdc.noaa.gov/nos/
Filters for Great Lakes surveys (41-49N, -92 to -76W)
Downloads BAG files to /data/cesarops/bathymetry/nos_surveys/
"""

import os
import re
import sys
import json
import time
import requests
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor, as_completed

# Great Lakes bounding box (generous)
GL_LAT_MIN, GL_LAT_MAX = 41.0, 49.0
GL_LON_MIN, GL_LON_MAX = -92.0, -76.0

BASE_URL = "https://www.ngdc.noaa.gov/nos"
DATA_URL = "https://data.ngdc.noaa.gov/platforms/ocean/nos/coast"
OUTPUT_DIR = Path("/data/cesarops/bathymetry/nos_surveys")
MANIFEST = OUTPUT_DIR / "manifest.json"

RANGES = [
    "H00001-H02000", "H02001-H04000", "H04001-H06000",
    "H06001-H08000", "H08001-H10000", "H10001-H12000",
    "H12001-H14000", "H14001-H16000",
]

session = requests.Session()
session.headers.update({"User-Agent": "CESAROPS-BAG-Scraper/1.0 (research)"})


def get_survey_ids(range_dir):
    """Fetch all survey IDs from a range directory."""
    url = f"{BASE_URL}/{range_dir}/"
    try:
        r = session.get(url, timeout=30)
        if r.status_code != 200:
            print(f"  [WARN] {url} returned {r.status_code}")
            return []
        # Parse HTML links like H12001.html
        ids = re.findall(r'(H\d{5})\.html', r.text)
        return sorted(set(ids))
    except Exception as e:
        print(f"  [ERR] {url}: {e}")
        return []


def check_survey_location(survey_id, range_dir):
    """Fetch survey metadata page and extract lat/lon. Return True if Great Lakes."""
    url = f"{BASE_URL}/{range_dir}/{survey_id}.html"
    try:
        r = session.get(url, timeout=30)
        if r.status_code != 200:
            return None

        # Look for coordinates in various formats
        # Pattern 1: "Latitude: 45.123" or "Lat: 45.123"
        lats = re.findall(r'(?:Latitude|Lat)[:\s]+(-?\d+\.?\d*)', r.text, re.IGNORECASE)
        lons = re.findall(r'(?:Longitude|Lon|Long)[:\s]+(-?\d+\.?\d*)', r.text, re.IGNORECASE)

        # Pattern 2: Bounding box "North: 45.5" "South: 45.0" "East: -84.0" "West: -85.0"
        north = re.findall(r'(?:North|north)[:\s]+(-?\d+\.?\d*)', r.text)
        south = re.findall(r'(?:South|south)[:\s]+(-?\d+\.?\d*)', r.text)
        east = re.findall(r'(?:East|east)[:\s]+(-?\d+\.?\d*)', r.text)
        west = re.findall(r'(?:West|west)[:\s]+(-?\d+\.?\d*)', r.text)

        # Try bounding box first
        if north and south and east and west:
            n, s = float(north[0]), float(south[0])
            e, w = float(east[0]), float(west[0])
            center_lat = (n + s) / 2
            center_lon = (e + w) / 2
        elif lats and lons:
            center_lat = float(lats[0])
            center_lon = float(lons[0])
        else:
            # Try "Great Lakes" or lake names in text
            gl_keywords = ['lake michigan', 'lake erie', 'lake huron', 'lake superior',
                          'lake ontario', 'straits of mackinac', 'green bay', 'saginaw',
                          'thunder bay', 'detroit river', 'st. clair', 'great lakes']
            text_lower = r.text.lower()
            if any(kw in text_lower for kw in gl_keywords):
                return {"survey_id": survey_id, "source": "keyword_match", "lat": 0, "lon": 0}
            return None

        # Check if in Great Lakes bbox
        if GL_LAT_MIN <= center_lat <= GL_LAT_MAX and GL_LON_MIN <= center_lon <= GL_LON_MAX:
            return {"survey_id": survey_id, "lat": center_lat, "lon": center_lon}

        return None
    except Exception as e:
        return None


def find_bag_urls(survey_id, range_dir):
    """Try to find BAG file download URLs for a survey."""
    # The BAG files are typically at:
    # https://data.ngdc.noaa.gov/platforms/ocean/nos/coast/{range}/{survey_id}/BAG/
    bag_dir_url = f"{DATA_URL}/{range_dir}/{survey_id}/BAG/"
    try:
        r = session.get(bag_dir_url, timeout=30)
        if r.status_code == 200:
            # Parse directory listing for .bag files
            bags = re.findall(r'href="([^"]+\.bag)"', r.text, re.IGNORECASE)
            if not bags:
                bags = re.findall(r'([\w_]+\.bag)', r.text, re.IGNORECASE)
            return [f"{bag_dir_url}{b}" for b in bags]
    except:
        pass

    # Try alternate path without /coast/
    alt_url = f"https://data.ngdc.noaa.gov/platforms/ocean/nos/{range_dir}/{survey_id}/BAG/"
    try:
        r = session.get(alt_url, timeout=30)
        if r.status_code == 200:
            bags = re.findall(r'href="([^"]+\.bag)"', r.text, re.IGNORECASE)
            if not bags:
                bags = re.findall(r'([\w_]+\.bag)', r.text, re.IGNORECASE)
            return [f"{alt_url}{b}" for b in bags]
    except:
        pass

    return []


def download_bag(url, output_dir):
    """Download a BAG file."""
    filename = url.split("/")[-1]
    filepath = output_dir / filename
    if filepath.exists():
        return filepath, "exists"

    try:
        r = session.get(url, timeout=300, stream=True)
        if r.status_code == 200:
            filepath.parent.mkdir(parents=True, exist_ok=True)
            with open(filepath, 'wb') as f:
                for chunk in r.iter_content(chunk_size=1024*1024):
                    f.write(chunk)
            size_mb = filepath.stat().st_size / (1024*1024)
            return filepath, f"downloaded ({size_mb:.1f} MB)"
        return None, f"HTTP {r.status_code}"
    except Exception as e:
        return None, str(e)


def main():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    # Load existing manifest if resuming
    manifest = {"surveys": [], "downloads": [], "errors": []}
    if MANIFEST.exists():
        manifest = json.loads(MANIFEST.read_text())

    already_checked = {s["survey_id"] for s in manifest["surveys"]}
    already_downloaded = {d["url"] for d in manifest["downloads"]}

    print(f"=== NCEI NOS Great Lakes BAG Scraper ===")
    print(f"Output: {OUTPUT_DIR}")
    print(f"Already checked: {len(already_checked)} surveys")
    print(f"Already downloaded: {len(already_downloaded)} files")
    print()

    total_found = 0
    total_downloaded = 0

    for range_dir in RANGES:
        print(f"\n[{range_dir}] Fetching survey index...")
        survey_ids = get_survey_ids(range_dir)
        print(f"  {len(survey_ids)} surveys in range")

        # Filter out already-checked
        to_check = [s for s in survey_ids if s not in already_checked]
        print(f"  {len(to_check)} new surveys to check")

        for i, survey_id in enumerate(to_check):
            if i % 50 == 0 and i > 0:
                print(f"  ... checked {i}/{len(to_check)}")
                # Save progress
                MANIFEST.write_text(json.dumps(manifest, indent=2))

            info = check_survey_location(survey_id, range_dir)
            if info:
                info["range"] = range_dir
                manifest["surveys"].append(info)
                total_found += 1
                print(f"  ✓ {survey_id} — Great Lakes ({info.get('lat',0):.2f}, {info.get('lon',0):.2f})")

                # Find and download BAG files
                bag_urls = find_bag_urls(survey_id, range_dir)
                for url in bag_urls:
                    if url in already_downloaded:
                        continue
                    filepath, status = download_bag(url, OUTPUT_DIR / survey_id)
                    manifest["downloads"].append({
                        "survey_id": survey_id,
                        "url": url,
                        "file": str(filepath) if filepath else None,
                        "status": status,
                    })
                    if filepath:
                        total_downloaded += 1
                        print(f"    ↓ {filepath.name} — {status}")

            already_checked.add(survey_id)
            time.sleep(0.2)  # Be polite to NOAA

        # Save after each range
        MANIFEST.write_text(json.dumps(manifest, indent=2))

    print(f"\n=== DONE ===")
    print(f"Great Lakes surveys found: {total_found}")
    print(f"BAG files downloaded: {total_downloaded}")
    print(f"Manifest: {MANIFEST}")


if __name__ == "__main__":
    main()
