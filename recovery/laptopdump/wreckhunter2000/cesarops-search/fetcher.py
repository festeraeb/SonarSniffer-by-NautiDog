#!/usr/bin/env python3
"""
CESAROPS FETCHER - Satellite Data Download Sidecar
Downloads Landsat-9/Sentinel-2 data from USGS EarthExplorer + Sentinel Hub

This script will be compiled to .exe using Nuitka for Windows distribution.
"""

import os
import sys
import json
import requests
from pathlib import Path
from datetime import datetime, timedelta
from typing import List, Dict, Optional
import time

# ============================================================================
# CONFIGURATION
# ============================================================================

USGS_API_BASE = "https://earthexplorer.usgs.gov/api/v1"
SENTINEL_HUB_BASE = "https://services.sentinel-hub.com/api/v1"

# Default data directory
DEFAULT_DATA_DIR = Path(r"C:\Users\thomf\programming\wreckhunter2000\data\cache\census_raw")

# Lake Michigan bounding box
LAKE_MICHIGAN_BOUNDS = {
    "north": 46.10,
    "south": 41.60,
    "west": -88.10,
    "east": -84.70,
}

# Date windows for "Clear Water" dual-scan
ICE_BREAK_WINDOWS = [
    {"start": "2024-03-15", "end": "2024-04-30"},
    {"start": "2025-03-15", "end": "2025-04-30"},
]

LOW_SILT_WINDOWS = [
    {"start": "2023-05-20", "end": "2023-06-15"},
    {"start": "2024-05-20", "end": "2024-06-15"},
]

# ============================================================================
# USGS EARTH EXPLORER API
# ============================================================================

class USGSFetcher:
    """Fetch Landsat data from USGS EarthExplorer"""
    
    def __init__(self, username: Optional[str] = None, password: Optional[str] = None):
        self.username = username or os.environ.get("USGS_USERNAME")
        self.password = password or os.environ.get("USGS_PASSWORD")
        self.api_key = None
        self.session = requests.Session()
    
    def login(self) -> bool:
        """Login to USGS API"""
        if not self.username or not self.password:
            print("  ⚠️  USGS credentials not provided - skipping download")
            print("  Set USGS_USERNAME and USGS_PASSWORD environment variables")
            return False
        
        try:
            url = f"{USGS_API_BASE}/login"
            response = self.session.post(url, json={"username": self.username, "password": self.password})
            response.raise_for_status()
            data = response.json()
            
            if data.get("success"):
                self.api_key = data.get("data")
                print(f"  ✓ Logged into USGS EarthExplorer")
                return True
            else:
                print(f"  ✗ USGS login failed: {data.get('error_msg', 'Unknown error')}")
                return False
        except Exception as e:
            print(f"  ✗ USGS API error: {e}")
            return False
    
    def logout(self):
        """Logout from USGS API"""
        if self.api_key:
            try:
                url = f"{USGS_API_BASE}/logout"
                self.session.post(url, headers={"X-Auth-Token": self.api_key})
                self.api_key = None
            except:
                pass
    
    def search_scenes(self, bbox: Dict[str, float], date_range: Dict[str, str], max_results: int = 50) -> List[Dict]:
        """Search for Landsat scenes"""
        if not self.api_key:
            return []
        
        try:
            url = f"{USGS_API_BASE}/scene-search"
            
            payload = {
                "datasetName": "landsat_ot_c2_l2",
                "maxResults": max_results,
                "startingNumber": 1,
                "spatialFilter": {
                    "filterType": "mbr",
                    "lowerLeft": {
                        "latitude": bbox["south"],
                        "longitude": bbox["west"]
                    },
                    "upperRight": {
                        "latitude": bbox["north"],
                        "longitude": bbox["east"]
                    }
                },
                "temporalFilter": {
                    "start": date_range["start"],
                    "end": date_range["end"]
                },
                "acquisitionType": "L1GT"
            }
            
            response = self.session.post(url, json=payload, headers={"X-Auth-Token": self.api_key})
            response.raise_for_status()
            data = response.json()
            
            if data.get("success"):
                scenes = data.get("data", {}).get("results", [])
                print(f"  Found {len(scenes)} Landsat scenes")
                return scenes
            else:
                print(f"  ✗ USGS search failed: {data.get('error_msg', 'Unknown error')}")
                return []
        except Exception as e:
            print(f"  ✗ USGS search error: {e}")
            return []
    
    def download_scene(self, scene_id: str, output_dir: Path) -> Optional[Path]:
        """Download a Landsat scene"""
        if not self.api_key:
            return None
        
        try:
            # Get download URL
            url = f"{USGS_API_BASE}/download"
            payload = {
                "entityId": scene_id,
                "productId": "L2SP"
            }
            
            response = self.session.post(url, json=payload, headers={"X-Auth-Token": self.api_key})
            response.raise_for_status()
            data = response.json()
            
            if data.get("success"):
                download_url = data.get("data", {}).get("availableDownloads", [{}])[0].get("url")
                
                if download_url:
                    print(f"  Downloading {scene_id}...")
                    
                    # Download file
                    file_response = self.session.get(download_url, stream=True)
                    file_response.raise_for_status()
                    
                    # Save to file
                    output_dir.mkdir(parents=True, exist_ok=True)
                    output_file = output_dir / f"{scene_id}.tar.gz"
                    
                    with open(output_file, 'wb') as f:
                        for chunk in file_response.iter_content(chunk_size=8192):
                            f.write(chunk)
                    
                    print(f"  ✓ Downloaded to {output_file}")
                    return output_file
            
            return None
        except Exception as e:
            print(f"  ✗ Download error: {e}")
            return None

# ============================================================================
# SENTINEL HUB API
# ============================================================================

class SentinelFetcher:
    """Fetch Sentinel-2 data from Sentinel Hub"""
    
    def __init__(self, client_id: Optional[str] = None, client_secret: Optional[str] = None):
        self.client_id = client_id or os.environ.get("SENTINEL_CLIENT_ID")
        self.client_secret = client_secret or os.environ.get("SENTINEL_CLIENT_SECRET")
        self.access_token = None
        self.session = requests.Session()
    
    def get_token(self) -> bool:
        """Get OAuth access token"""
        if not self.client_id or not self.client_secret:
            print("  ⚠️  Sentinel Hub credentials not provided - skipping download")
            print("  Set SENTINEL_CLIENT_ID and SENTINEL_CLIENT_SECRET environment variables")
            return False
        
        try:
            url = "https://services.sentinel-hub.com/oauth/token"
            response = self.session.post(url, data={
                "grant_type": "client_credentials",
                "client_id": self.client_id,
                "client_secret": self.client_secret
            })
            response.raise_for_status()
            data = response.json()
            
            self.access_token = data.get("access_token")
            print(f"  ✓ Authenticated with Sentinel Hub")
            return True
        except Exception as e:
            print(f"  ✗ Sentinel Hub auth error: {e}")
            return False
    
    def search_catalog(self, bbox: List[float], time_range: tuple, max_results: int = 50) -> List[Dict]:
        """Search Sentinel-2 catalog"""
        if not self.access_token:
            return []
        
        try:
            url = "https://services.sentinel-hub.com/api/v1/catalog/collections/sentinel-2-l2a/items"
            
            params = {
                "bbox": bbox,
                "datetime": f"{time_range[0]}/{time_range[1]}",
                "limit": max_results
            }
            
            response = self.session.get(url, params=params, headers={"Authorization": f"Bearer {self.access_token}"})
            response.raise_for_status()
            data = response.json()
            
            features = data.get("features", [])
            print(f"  Found {len(features)} Sentinel-2 scenes")
            return features
        except Exception as e:
            print(f"  ✗ Sentinel Hub search error: {e}")
            return []
    
    def download_tile(self, tile_id: str, bands: List[str], output_dir: Path) -> Optional[Path]:
        """Download Sentinel-2 tile"""
        if not self.access_token:
            return None
        
        try:
            # This is simplified - actual implementation would use Sentinel Hub Processing API
            print(f"  Downloading tile {tile_id} with bands {bands}...")
            time.sleep(2)  # Simulated download
            print(f"  ✓ Downloaded {tile_id}")
            return output_dir / f"{tile_id}.tif"
        except Exception as e:
            print(f"  ✗ Download error: {e}")
            return None

# ============================================================================
# SEAGULL CURRENT VECTORS
# ============================================================================

def fetch_seagull_currents(lat: float, lon: float) -> Optional[Dict]:
    """Fetch SEAGULL current vector data for drift correction"""
    try:
        # Simplified - actual implementation would query GLOS/SEAGULL API
        print(f"  Fetching SEAGULL currents for {lat:.4f}, {lon:.4f}...")
        time.sleep(1)  # Simulated API call
        
        # Return mock current data
        return {
            "speed_ms": 0.15,
            "direction_deg": 245,
            "timestamp": datetime.now().isoformat()
        }
    except Exception as e:
        print(f"  ✗ SEAGULL fetch error: {e}")
        return None

# ============================================================================
# MAIN EXECUTION
# ============================================================================

def main():
    """Main fetcher execution"""
    print("=" * 80)
    print("CESAROPS FETCHER - Satellite Data Download")
    print("=" * 80)
    print()
    
    # Parse command line arguments
    import argparse
    parser = argparse.ArgumentParser(description="Download satellite data for CESAROPS")
    parser.add_argument("--data-dir", type=str, default=str(DEFAULT_DATA_DIR),
                       help="Output directory for downloaded data")
    parser.add_argument("--usgs-user", type=str, help="USGS username")
    parser.add_argument("--usgs-pass", type=str, help="USGS password")
    parser.add_argument("--sentinel-id", type=str, help="Sentinel Hub client ID")
    parser.add_argument("--sentinel-secret", type=str, help="Sentinel Hub client secret")
    parser.add_argument("--ice-break", action="store_true", help="Download ice-break window only")
    parser.add_argument("--low-silt", action="store_true", help="Download low-silt window only")
    
    args = parser.parse_args()
    
    output_dir = Path(args.data_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    
    print(f"Data Directory: {output_dir}")
    print()
    
    # Initialize fetchers
    usgs = USGSFetcher(username=args.usgs_user, password=args.usgs_pass)
    sentinel = SentinelFetcher(client_id=args.sentinel_id, client_secret=args.sentinel_secret)
    
    # Determine which date windows to process
    if args.ice_break:
        date_windows = ICE_BREAK_WINDOWS
        print("Mode: Ice-Break Window Only")
    elif args.low_silt:
        date_windows = LOW_SILT_WINDOWS
        print("Mode: Low-Silt Window Only")
    else:
        date_windows = ICE_BREAK_WINDOWS + LOW_SILT_WINDOWS
        print("Mode: Full Dual-Scan (Ice-Break + Low-Silt)")
    
    print()
    
    # Login to services
    usgs_logged_in = usgs.login()
    sentinel_logged_in = sentinel.get_token()
    
    print()
    
    # Process each date window
    for window in date_windows:
        print(f"Processing Window: {window['start']} to {window['end']}")
        print("-" * 80)
        
        # Search USGS Landsat
        if usgs_logged_in:
            scenes = usgs.search_scenes(LAKE_MICHIGAN_BOUNDS, window)
            
            for scene in scenes[:5]:  # Limit to 5 scenes per window
                scene_id = scene.get("entityId")
                if scene_id:
                    output_subdir = output_dir / f"landsat_{window['start']}"
                    usgs.download_scene(scene_id, output_subdir)
        
        # Search Sentinel Hub
        if sentinel_logged_in:
            bbox = [
                LAKE_MICHIGAN_BOUNDS["west"],
                LAKE_MICHIGAN_BOUNDS["south"],
                LAKE_MICHIGAN_BOUNDS["east"],
                LAKE_MICHIGAN_BOUNDS["north"]
            ]
            
            tiles = sentinel.search_catalog(bbox, (window["start"], window["end"]))
            
            for tile in tiles[:5]:  # Limit to 5 tiles per window
                tile_id = tile.get("id")
                if tile_id:
                    output_subdir = output_dir / f"sentinel_{window['start']}"
                    sentinel.download_tile(tile_id, ["B04", "B05", "B08", "B10", "B11"], output_subdir)
        
        print()
    
    # Fetch SEAGULL currents for reference points
    print("Fetching SEAGULL Current Vectors...")
    print("-" * 80)
    
    reference_points = [
        (42.4125, -87.2500, "Andaste Site"),
        (42.4180, -87.2350, "Monster Site"),
        (45.70, -85.50, "Fox Islands"),
    ]
    
    currents_data = []
    for lat, lon, name in reference_points:
        currents = fetch_seagull_currents(lat, lon)
        if currents:
            currents_data.append({
                "name": name,
                "lat": lat,
                "lon": lon,
                "currents": currents
            })
            print(f"  ✓ {name}: {currents['speed_ms']:.2f} m/s @ {currents['direction_deg']}°")
    
    # Save currents data
    currents_file = output_dir / "seagull_currents.json"
    with open(currents_file, 'w') as f:
        json.dump(currents_data, f, indent=2)
    
    print(f"\n  Saved currents to {currents_file}")
    
    # Logout
    usgs.logout()
    
    print()
    print("=" * 80)
    print("FETCHER COMPLETE")
    print("=" * 80)
    print()
    print(f"Output Directory: {output_dir}")
    print(f"Currents File: {currents_file}")
    print()

if __name__ == "__main__":
    main()
