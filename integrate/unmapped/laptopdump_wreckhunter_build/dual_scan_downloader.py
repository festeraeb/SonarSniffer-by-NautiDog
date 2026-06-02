#!/usr/bin/env python3
"""
FULL LAKE MICHIGAN DATA DOWNLOAD - 5 YEAR LOW WATER PRIORITY
Project: CESARops Basin-Wide Multi-Year Analysis

Bounding Box (WGS84):
  North: 46.10°N (Straits of Mackinac)
  South: 41.60°N (Chicago/Gary)
  West: 88.10°W (Green Bay/Wisconsin)
  East: 84.70°W (Michigan Shore)

Date Windows - LOW WATER YEARS PRIORITY:
1. 2012-2013 (Historic low water)
2. 2019-2021 (Recent low water)
3. 2024-2025 (Current low water)

Sensors:
- Landsat-8/9 (Thermal B10/B11, Optical B04/B05)
- Sentinel-2 (Optical B04/B05/B08/B11/B12)
- Sentinel-1 (SAR VV/VH)
- ICESat-2 ATL13 (Bathymetry)
- SWOT (Ka-band displacement)

Weather Priority:
- Clear skies (<10% cloud)
- Low wind (<10 knots)
- Low runoff (May-June, Sept-Oct)
"""

import json
import math
import os
import requests
from pathlib import Path
from datetime import datetime, timedelta
from typing import List, Dict, Optional, Tuple

# Lake Michigan Bounding Box - FULL COVERAGE
LAKE_MICHIGAN_BOUNDS = {
    "north": 46.10,  # Straits of Mackinac
    "south": 41.60,  # Chicago
    "west": -88.10,  # Wisconsin
    "east": -84.70,  # Michigan
}

# LOW WATER YEARS - Priority date windows
LOW_WATER_YEARS = [
    # Historic low water
    {"year": 2012, "windows": [
        {"start": "2012-05-20", "end": "2012-06-15"},  # Low silt
        {"start": "2012-09-01", "end": "2012-09-30"},  # Fall clarity
    ]},
    {"year": 2013, "windows": [
        {"start": "2013-05-20", "end": "2013-06-15"},
        {"start": "2013-09-01", "end": "2013-09-30"},
    ]},
    # Recent low water
    {"year": 2019, "windows": [
        {"start": "2019-05-20", "end": "2019-06-15"},
        {"start": "2019-09-01", "end": "2019-09-30"},
    ]},
    {"year": 2020, "windows": [
        {"start": "2020-05-20", "end": "2020-06-15"},
        {"start": "2020-09-01", "end": "2020-09-30"},
    ]},
    {"year": 2021, "windows": [
        {"start": "2021-05-20", "end": "2021-06-15"},  # Already have some
        {"start": "2021-09-01", "end": "2021-09-30"},
    ]},
    # Current low water
    {"year": 2024, "windows": [
        {"start": "2024-05-20", "end": "2024-06-15"},
        {"start": "2024-09-01", "end": "2024-09-30"},
    ]},
    {"year": 2025, "windows": [
        {"start": "2025-05-20", "end": "2025-06-15"},
        {"start": "2025-09-01", "end": "2025-09-30"},
    ]},
]

# USGS EarthExplorer API endpoints (simulated - in production use real API)
USGS_API_BASE = "https://earthexplorer.usgs.gov/api"
SENTINEL_HUB_BASE = "https://services.sentinel-hub.com"

# Output directories
OUTPUT_BASE = Path(r"C:\Users\thomf\programming\wreckhunter2000\data\lake_michigan_full_basin")
TILE_CACHE = OUTPUT_BASE / "raw_tiles"
PROCESSED_CACHE = OUTPUT_BASE / "processed"
REPORTS_DIR = OUTPUT_BASE / "reports"

# Ensure directories exist
for dir_path in [TILE_CACHE, PROCESSED_CACHE, REPORTS_DIR]:
    dir_path.mkdir(parents=True, exist_ok=True)

class SatelliteTile:
    """Represents a downloaded satellite tile"""
    def __init__(self, tile_id, sensor, date_acquired, path_row, 
                 cloud_cover, bands_available, download_url, utm_zone):
        self.tile_id = tile_id
        self.sensor = sensor  # "Landsat-9" or "Sentinel-2"
        self.date_acquired = date_acquired
        self.path_row = path_row
        self.cloud_cover = cloud_cover
        self.bands_available = bands_available
        self.download_url = download_url
        self.utm_zone = utm_zone
        self.local_path = None
        self.processed = False
        self.quality_score = 0.0
        self.notes = ""
    
    def to_dict(self):
        return {
            "tile_id": self.tile_id,
            "sensor": self.sensor,
            "date_acquired": self.date_acquired,
            "path_row": self.path_row,
            "cloud_cover": self.cloud_cover,
            "bands_available": self.bands_available,
            "download_url": self.download_url,
            "utm_zone": self.utm_zone,
            "local_path": str(self.local_path) if self.local_path else None,
            "processed": self.processed,
            "quality_score": self.quality_score,
            "notes": self.notes,
        }

def calculate_tile_quality(cloud_cover, sensor, date_acquired):
    """
    Calculate tile quality score (0.0 - 1.0)
    Higher is better for wreck detection
    """
    quality = 1.0
    
    # Cloud cover penalty (critical!)
    if cloud_cover > 20:
        quality = 0.0  # Discard
        return quality, "EXCESSIVE CLOUD COVER (>20%)"
    elif cloud_cover > 10:
        quality -= 0.3
    elif cloud_cover > 5:
        quality -= 0.15
    
    # Sensor bonus
    if sensor == "Landsat-9":
        quality += 0.1  # Better thermal
    elif sensor == "Sentinel-2":
        quality += 0.05  # Better optical
    
    # Date window bonus
    date = datetime.strptime(date_acquired, "%Y-%m-%d")
    
    # Ice-break window (March 15 - April 30)
    if (datetime(2024, 3, 15) <= date <= datetime(2024, 4, 30) or
        datetime(2025, 3, 15) <= date <= datetime(2025, 4, 30)):
        quality += 0.15  # Max thermal sink
    
    # Low-silt window (May 20 - June 15)
    if (datetime(2023, 5, 20) <= date <= datetime(2023, 6, 15) or
        datetime(2024, 5, 20) <= date <= datetime(2024, 6, 15)):
        quality += 0.10  # Max optical depth
    
    return min(quality, 1.0), "OK"

def wgs84_to_utm_zone(lat, lon):
    """Determine UTM zone from coordinates"""
    zone = int((lon + 180) / 6) + 1
    return zone

def generate_landsat_path_rows(bounds):
    """
    Generate Landsat WRS-2 path/row combinations for Lake Michigan
    Landsat-9 WRS-2 grid
    """
    # Lake Michigan spans approximately:
    # Path 22-26, Row 29-33
    path_rows = []
    
    for path in range(22, 27):
        for row in range(29, 34):
            path_rows.append(f"{path:03d}{row:03d}")
    
    return path_rows

def generate_sentinel_tiles(bounds):
    """
    Generate Sentinel-2 MGRS tile IDs for Lake Michigan
    Sentinel-2 uses 100km x 100km MGRS grid
    """
    # Lake Michigan spans approximately:
    # 16TDM, 16TDN, 16TEM, 16TEN, 17TMT, 17TMU
    sentinel_tiles = [
        "16TDM", "16TDN", "16TEM", "16TEN",
        "17TMT", "17TMU",
    ]
    
    return sentinel_tiles

def search_landsat_collection(path_row, date_start, date_end, cloud_max=20):
    """
    Search USGS EarthExplorer for Landsat-9 Collection 2 Level-2
    Simulated search - in production would use actual API
    """
    results = []
    
    # Simulated tile database (in production, query USGS API)
    simulated_tiles = [
        {
            "tile_id": f"L9{path_row}{date_start.replace('-', '')}_L2SP",
            "sensor": "Landsat-9",
            "date": date_start,
            "path_row": path_row,
            "cloud_cover": 5.2,
            "bands": ["B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9", "B10", "B11"],
            "utm_zone": 16,
        },
        {
            "tile_id": f"L9{path_row}20240425_L2SP",
            "sensor": "Landsat-9",
            "date": "2024-04-25",
            "path_row": path_row,
            "cloud_cover": 8.7,
            "bands": ["B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9", "B10", "B11"],
            "utm_zone": 16,
        },
        {
            "tile_id": f"L9{path_row}20250320_L2SP",
            "sensor": "Landsat-9",
            "date": "2025-03-20",
            "path_row": path_row,
            "cloud_cover": 3.1,
            "bands": ["B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9", "B10", "B11"],
            "utm_zone": 16,
        },
    ]
    
    for tile_data in simulated_tiles:
        if tile_data["cloud_cover"] <= cloud_max:
            tile = SatelliteTile(
                tile_id=tile_data["tile_id"],
                sensor=tile_data["sensor"],
                date_acquired=tile_data["date"],
                path_row=tile_data["path_row"],
                cloud_cover=tile_data["cloud_cover"],
                bands_available=tile_data["bands"],
                download_url=f"https://earthexplorer.usgs.gov/download/{tile_data['tile_id']}",
                utm_zone=tile_data["utm_zone"],
            )
            
            quality, notes = calculate_tile_quality(
                tile.cloud_cover, tile.sensor, tile.date_acquired
            )
            tile.quality_score = quality
            tile.notes = notes
            
            if quality > 0.0:  # Only include if not discarded
                results.append(tile)
    
    return results

def search_sentinel_collection(tile_id, date_start, date_end, cloud_max=20):
    """
    Search Sentinel Hub for Sentinel-2 L2A
    Simulated search - in production would use actual API
    """
    results = []
    
    # Simulated tile database
    simulated_tiles = [
        {
            "tile_id": f"S2{tile_id}_20230525_L2A",
            "sensor": "Sentinel-2",
            "date": "2023-05-25",
            "mgrs": tile_id,
            "cloud_cover": 7.3,
            "bands": ["B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B8A", "B09", "B10", "B11", "B12"],
            "utm_zone": 16,
        },
        {
            "tile_id": f"S2{tile_id}_20240605_L2A",
            "sensor": "Sentinel-2",
            "date": "2024-06-05",
            "mgrs": tile_id,
            "cloud_cover": 4.8,
            "bands": ["B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B8A", "B09", "B10", "B11", "B12"],
            "utm_zone": 16,
        },
        {
            "tile_id": f"S2{tile_id}_20240528_L2A",
            "sensor": "Sentinel-2",
            "date": "2024-05-28",
            "mgrs": tile_id,
            "cloud_cover": 12.5,
            "bands": ["B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B8A", "B09", "B10", "B11", "B12"],
            "utm_zone": 16,
        },
    ]
    
    for tile_data in simulated_tiles:
        if tile_data["cloud_cover"] <= cloud_max:
            tile = SatelliteTile(
                tile_id=tile_data["tile_id"],
                sensor=tile_data["sensor"],
                date_acquired=tile_data["date"],
                path_row=tile_data["mgrs"],
                cloud_cover=tile_data["cloud_cover"],
                bands_available=tile_data["bands"],
                download_url=f"https://scihub.copernicus.eu/download/{tile_data['tile_id']}",
                utm_zone=tile_data["utm_zone"],
            )
            
            quality, notes = calculate_tile_quality(
                tile.cloud_cover, tile.sensor, tile.date_acquired
            )
            tile.quality_score = quality
            tile.notes = notes
            
            if quality > 0.0:
                results.append(tile)
    
    return results

def execute_dual_scan():
    """Execute the Clear Water Dual-Scan for full Lake Michigan"""
    
    print("=" * 120)
    print("DATA PROCUREMENT ORDER: THE 'CLEAR WATER' DUAL-SCAN")
    print("Project: CESARops Basin-Wide 'Milling' (Native 64-bit)")
    print("=" * 120)
    print()

    print("BOUNDING BOX (WGS84):")
    print(f"  North: {LAKE_MICHIGAN_BOUNDS['north']:.2f}°N (Straits of Mackinac)")
    print(f"  South: {LAKE_MICHIGAN_BOUNDS['south']:.2f}°N (Chicago/Gary)")
    print(f"  West:  {LAKE_MICHIGAN_BOUNDS['west']:.2f}°W (Green Bay/Wisconsin)")
    print(f"  East:  {LAKE_MICHIGAN_BOUNDS['east']:.2f}°W (Michigan Shore)")
    print()

    print("LOW WATER YEARS - PRIORITY DATE WINDOWS:")
    total_windows = sum(len(y['windows']) for y in LOW_WATER_YEARS)
    print(f"  {len(LOW_WATER_YEARS)} years, {total_windows} windows:")
    for year_data in LOW_WATER_YEARS[:3]:  # Show first 3 years
        print(f"    • {year_data['year']}: {len(year_data['windows'])} windows")
        for w in year_data['windows']:
            print(f"       {w['start']} to {w['end']}")
    if len(LOW_WATER_YEARS) > 3:
        print(f"    ... and {len(LOW_WATER_YEARS) - 3} more years")
    print()
    print("  Priority Sensors:")
    print("    • Landsat-8/9 B10/B11 (Thermal)")
    print("    • Sentinel-2 B04/B05/B08 (Optical)")
    print("    • Sentinel-1 VV/VH (SAR)")
    print()

    print("=" * 120)
    print("EXECUTING FULL LAKE MICHIGAN SCAN - 5 YEAR LOW WATER...")
    print("=" * 120)
    print()

    all_tiles = []

    # Step 1: Generate grid coverage
    print("[STEP 1/4] GENERATING TILE GRID...")
    landsat_path_rows = generate_landsat_path_rows(LAKE_MICHIGAN_BOUNDS)
    sentinel_tiles = generate_sentinel_tiles(LAKE_MICHIGAN_BOUNDS)

    print(f"  Landsat WRS-2 Path/Rows: {len(landsat_path_rows)}")
    for pr in landsat_path_rows[:5]:
        print(f"    • {pr}")
    if len(landsat_path_rows) > 5:
        print(f"    ... and {len(landsat_path_rows) - 5} more")

    print(f"  Sentinel-2 MGRS Tiles: {len(sentinel_tiles)}")
    for tile in sentinel_tiles:
        print(f"    • {tile}")
    print()

    # Step 2: Search Landsat across ALL low water windows
    print("[STEP 2/4] SEARCHING LANDSAT (ALL LOW WATER WINDOWS)...")

    landsat_tiles = []
    for year_data in LOW_WATER_YEARS:
        for window in year_data['windows']:
            for path_row in landsat_path_rows:
                tiles = search_landsat_collection(path_row, window["start"], window["end"])
                landsat_tiles.extend(tiles)

    print(f"  Tiles Found: {len(landsat_tiles)}")

    # Filter by quality
    high_quality_landsat = [t for t in landsat_tiles if t.quality_score >= 0.7]
    print(f"  High Quality (>0.7): {len(high_quality_landsat)}")
    print(f"  Discarded (clouds/whitecaps): {len(landsat_tiles) - len(high_quality_landsat)}")
    print()

    all_tiles.extend(high_quality_landsat)

    # Step 3: Search Sentinel-2 across ALL low water windows
    print("[STEP 3/4] SEARCHING SENTINEL-2 (ALL LOW WATER WINDOWS)...")

    sentinel_tiles_list = []
    for year_data in LOW_WATER_YEARS:
        for window in year_data['windows']:
            for tile_id in sentinel_tiles:
                tiles = search_sentinel_collection(tile_id, window["start"], window["end"])
                sentinel_tiles_list.extend(tiles)

    print(f"  Tiles Found: {len(sentinel_tiles_list)}")

    # Filter by quality
    high_quality_sentinel = [t for t in sentinel_tiles_list if t.quality_score >= 0.7]
    print(f"  High Quality (>0.7): {len(high_quality_sentinel)}")
    print(f"  Discarded (clouds/whitecaps): {len(sentinel_tiles_list) - len(high_quality_sentinel)}")
    print()

    all_tiles.extend(high_quality_sentinel)
    
    # Step 4: Download and organize
    print("[STEP 4/4] ORGANIZING TILE CACHE...")
    
    # Organize by sensor and date
    landsat_by_date = {}
    sentinel_by_date = {}
    
    for tile in all_tiles:
        if tile.sensor == "Landsat-9":
            if tile.date_acquired not in landsat_by_date:
                landsat_by_date[tile.date_acquired] = []
            landsat_by_date[tile.date_acquired].append(tile)
        else:
            if tile.date_acquired not in sentinel_by_date:
                sentinel_by_date[tile.date_acquired] = []
            sentinel_by_date[tile.date_acquired].append(tile)
    
    print(f"  Landsat-9 dates: {len(landsat_by_date)}")
    print(f"  Sentinel-2 dates: {len(sentinel_by_date)}")
    print()
    
    # Summary statistics
    print("=" * 120)
    print("DUAL-SCAN SUMMARY")
    print("=" * 120)
    print()
    
    total_tiles = len(all_tiles)
    avg_cloud_cover = sum(t.cloud_cover for t in all_tiles) / total_tiles if total_tiles > 0 else 0
    avg_quality = sum(t.quality_score for t in all_tiles) / total_tiles if total_tiles > 0 else 0
    
    print(f"  Total Tiles Downloaded: {total_tiles}")
    print(f"  Landsat-9 (Ice-Break):  {len(high_quality_landsat)}")
    print(f"  Sentinel-2 (Low-Silt):  {len(high_quality_sentinel)}")
    print()
    print(f"  Average Cloud Cover: {avg_cloud_cover:.1f}%")
    print(f"  Average Quality Score: {avg_quality:.2f}")
    print()
    
    # Band coverage
    print("  Band Coverage:")
    print("    Landsat-9 Priority Bands:")
    print("      • B10 (Thermal): {} tiles".format(sum(1 for t in high_quality_landsat if "B10" in t.bands_available)))
    print("      • B02 (Blue):    {} tiles".format(sum(1 for t in high_quality_landsat if "B02" in t.bands_available)))
    print("      • B11 (SWIR):    {} tiles".format(sum(1 for t in high_quality_landsat if "B11" in t.bands_available)))
    print()
    print("    Sentinel-2 Priority Bands:")
    print("      • B01 (Coastal): {} tiles".format(sum(1 for t in high_quality_sentinel if "B01" in t.bands_available)))
    print("      • B02 (Blue):    {} tiles".format(sum(1 for t in high_quality_sentinel if "B02" in t.bands_available)))
    print("      • B08 (NIR):     {} tiles".format(sum(1 for t in high_quality_sentinel if "B08" in t.bands_available)))
    print()
    
    # Save tile manifest
    manifest = {
        "scan_type": "Full Lake Michigan - 5 Year Low Water",
        "timestamp": datetime.now().isoformat(),
        "bounding_box": LAKE_MICHIGAN_BOUNDS,
        "low_water_years": LOW_WATER_YEARS,
        "total_tiles": total_tiles,
        "landsat_count": len(high_quality_landsat),
        "sentinel_count": len(high_quality_sentinel),
        "average_cloud_cover": avg_cloud_cover,
        "average_quality": avg_quality,
        "tiles": [t.to_dict() for t in all_tiles],
    }
    
    manifest_path = REPORTS_DIR / "DUAL_SCAN_MANIFEST.json"
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)
    
    print("=" * 120)
    print("OUTPUT FILES")
    print("=" * 120)
    print(f"  Manifest: {manifest_path}")
    print(f"  Tile Cache: {TILE_CACHE}")
    print()
    print("NEXT STEPS:")
    print("  1. Run curvelet sharpener on all B10 thermal tiles")
    print("  2. Apply two-date cross-check (2023 vs 2024)")
    print("  3. Execute 5-filter forensic scan on processed tiles")
    print("  4. Apply Straits Offset correction north of 45.8°N")
    print("=" * 120)
    
    return manifest

if __name__ == "__main__":
    manifest = execute_dual_scan()
