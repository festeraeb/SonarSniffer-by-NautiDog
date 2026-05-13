import os
import requests
import json
import logging
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor, as_completed

logging.basicConfig(level=logging.INFO, format='%(asctime)s [%(levelname)s] %(message)s')

REPO_DIR = Path(__file__).resolve().parent
ENV_FILE = REPO_DIR / ".env"
OUTPUT_DIR = REPO_DIR / "downloads" / "erie_central_basin"

def load_token():
    if not ENV_FILE.exists(): return None
    for line in ENV_FILE.read_text().splitlines():
        if "EARTHDATA_TOKEN" in line and "=" in line:
            return line.split("=", 1)[1].strip()
    return None

TOKEN = load_token()
SESSION = requests.Session()
if TOKEN:
    SESSION.headers.update({"Authorization": f"Bearer {TOKEN}"})
SESSION.headers.update({"Accept": "application/json"})

# Central Basin of Lake Erie (Approximate)
BBOX = "-82.4,41.5,-80.5,42.5"

# All optical bands relevant to water penetration and surface anomaly detection
# B01 = Coastal Blue (Deepest penetration in clear water)
# B02 = Blue (Standard Stumpf)
# B03 = Green (Standard Stumpf)
# B04 = Red (Shallow water / surface)
# B08 = NIR (Surface boundary / wake detection)
TARGET_BANDS = ["B01.tif", "B02.tif", "B03.tif", "B04.tif", "B08.tif", "Fmask.tif"]

def search_cmr(bbox, max_granules=10):
    url = "https://cmr.earthdata.nasa.gov/search/granules.json"
    params = {
        "short_name": "HLSS30",
        "version": "2.0",
        "bounding_box": bbox,
        "temporal": "2023-06-01T00:00:00Z,2023-08-31T23:59:59Z", # Summer for best light
        "page_size": 50,
        "cloud_cover": "0,20" # Strict cloud cover
    }
    
    resp = requests.get(url, params=params)
    resp.raise_for_status()
    data = resp.json()
    
    granules = data.get("feed", {}).get("entry", [])
    if not granules: return []
    
    results = []
    for g in granules[:max_granules]:
        g_name = g["title"]
        links = [link["href"] for link in g.get("links", []) 
                 if link["href"].endswith(".tif") 
                 and any(b in link["href"] for b in TARGET_BANDS)
                 and link["href"].startswith("https://")]
        
        if links:
            results.append({"name": g_name, "links": links})
            
    return results

def download_file(url, out_path):
    if out_path.exists() and out_path.stat().st_size > 10000:
        return True
    out_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with SESSION.get(url, stream=True, timeout=30) as r:
            r.raise_for_status()
            with open(out_path, 'wb') as f:
                for chunk in r.iter_content(chunk_size=8192):
                    f.write(chunk)
        return True
    except Exception as e:
        logging.error(f"Failed {url}: {e}")
        return False

def main():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    logging.info(f"Searching CMR for Erie Central Basin: {BBOX}")
    
    granules = search_cmr(BBOX, max_granules=15)
    logging.info(f"Found {len(granules)} clear granules. Downloading...")
    
    tasks = []
    with ThreadPoolExecutor(max_workers=8) as pool:
        for g in granules:
            g_dir = OUTPUT_DIR / g["name"]
            for link in g["links"]:
                filename = link.split("/")[-1]
                out_path = g_dir / filename
                tasks.append(pool.submit(download_file, link, out_path))
                
        for i, fut in enumerate(as_completed(tasks)):
            if i % 10 == 0:
                logging.info(f"Downloaded {i}/{len(tasks)} files...")
                
    logging.info("Lake Erie Central Basin expansion complete!")

if __name__ == "__main__":
    main()
