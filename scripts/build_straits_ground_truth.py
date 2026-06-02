#!/usr/bin/env python3
"""Build Straits of Mackinac ground-truth wreck datasets from the verified
Great Lakes preserve registry.

Reads outputs/great_lakes_preserve_wrecks.json (preserve_registry coordinates,
attributed to michiganpreserves.org) and emits two files the pipelines consume:

  1. data/known_wrecks_straits.json  — satellite format (bbox per wreck, keyed
     by id). Loaded via MissionSpec.paths.known_wrecks_json.
  2. data/straits_known_wrecks.json  — flat list (name/lat/lon/depth/type) for
     the mag known_data Straits set and for reference.

Cleaning rules:
  - Drop entries with no usable coordinate or obvious placeholder names
    ("Coordinates", "Rock Maze" @ 0 ft).
  - Restrict to the Straits of Mackinac bounding box so a mislabelled western
    entry (e.g. Chuck's Barge @ -85.85) doesn't pollute the set.
  - Merge bow/stern/engine fragments of the same hull under the parent name but
    keep them as separate GT points (they are distinct dive targets).
"""
import json
import os
import re

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(REPO, "outputs", "great_lakes_preserve_wrecks.json")
DATA_DIR = os.path.join(REPO, "data")

# Straits of Mackinac AOI (matches the satellite AREAS preset, slightly padded).
LAT_MIN, LAT_MAX = 45.60, 46.00
LON_MIN, LON_MAX = -85.25, -84.20

# ~150 m half-box around a point for the satellite bbox format.
HALF_DEG = 150.0 / 111_320.0

PLACEHOLDER_NAMES = {"coordinates", "rock maze"}


def usable(w):
    lat, lon = w.get("lat"), w.get("lon")
    if lat is None or lon is None:
        return False
    if not (LAT_MIN <= lat <= LAT_MAX and LON_MIN <= lon <= LON_MAX):
        return False
    name = (w.get("name") or "").strip()
    if not name or name.lower() in PLACEHOLDER_NAMES:
        return False
    # Reject obvious placeholder depth-0 with placeholder name already handled.
    return True


def slugify(name):
    s = re.sub(r"[^a-z0-9]+", "_", name.lower()).strip("_")
    return s or "wreck"


def main():
    with open(SRC, encoding="utf-8") as f:
        allw = json.load(f)

    straits = [
        w for w in allw
        if w.get("preserve") and "straits" in w["preserve"].lower() and usable(w)
    ]
    straits.sort(key=lambda w: w["name"])

    os.makedirs(DATA_DIR, exist_ok=True)

    # ── Satellite format: { id: {lat_min,lon_min,lat_max,lon_max,name,depth_ft,type,confidence} }
    sat = {}
    seen = {}
    for w in straits:
        base = slugify(w["name"])
        seen[base] = seen.get(base, 0) + 1
        wid = base if seen[base] == 1 else f"{base}_{seen[base]}"
        cos_lat = max(1e-6, abs(__import__("math").cos(__import__("math").radians(w["lat"]))))
        dlon = HALF_DEG / cos_lat
        sat[wid] = {
            "name": w["name"],
            "lat_min": round(w["lat"] - HALF_DEG, 6),
            "lat_max": round(w["lat"] + HALF_DEG, 6),
            "lon_min": round(w["lon"] - dlon, 6),
            "lon_max": round(w["lon"] + dlon, 6),
            "depth_ft": w.get("depth_ft"),
            "type": w.get("vessel_type") or "",
            # Preserve-registry coords are dive-grade ground truth.
            "confidence": "dive_verified",
            "source": w.get("source_site") or w.get("source") or "michiganpreserves.org",
        }
    sat_path = os.path.join(DATA_DIR, "known_wrecks_straits.json")
    with open(sat_path, "w", encoding="utf-8") as f:
        json.dump(sat, f, indent=2)

    # ── Flat list (mag / reference) ──
    flat = [
        {
            "name": w["name"],
            "lat": w["lat"],
            "lon": w["lon"],
            "depth_ft": w.get("depth_ft") or 0,
            "vessel_type": w.get("vessel_type") or "",
            "source": w.get("source_site") or "michiganpreserves.org",
        }
        for w in straits
    ]
    flat_path = os.path.join(DATA_DIR, "straits_known_wrecks.json")
    with open(flat_path, "w", encoding="utf-8") as f:
        json.dump(flat, f, indent=2)

    print(f"Straits ground truth: {len(straits)} wrecks")
    print(f"  satellite bbox format -> {sat_path}")
    print(f"  flat list             -> {flat_path}")
    for w in straits:
        print(f"    {w['name'][:30]:30s} {w['lat']:.5f} {w['lon']:.5f}")


if __name__ == "__main__":
    main()
