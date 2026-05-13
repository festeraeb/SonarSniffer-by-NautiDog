import httpx
from bs4 import BeautifulSoup
import json
import os
import re

URL_BASE = "http://www.eriewrecks.com/shipwrecks/"
PAGES = [
    "shipwrecks.html",
    "wefound/wefound.html",
    "westbasin/westbasin.html",
    "huron_avonpoint/huron_avonpoint.html",
    "cleveland/cleveland.html",
    "longpoint/longpoint.html"
]

OUTPUT_FILE = "known_wrecks_erie.json"

def fetch_erie_wrecks():
    print("Scraping Erie Wrecks...")
    wrecks = []
    
    for page in PAGES:
        try:
            url = f"{URL_BASE}{page}"
            response = httpx.get(url, timeout=10.0)
            response.raise_for_status()
            
            soup = BeautifulSoup(response.text, "html.parser")
            
            # Simple heuristic: Look for links that might represent wreck entries
            links = soup.find_all("a", href=True)
            for link in links:
                text_content = link.get_text(strip=True)
                # Quick filter for ships by looking for 'Lost', 'Sank', or common wreck indicators
                if ("Lost" in text_content or "Stmr" in text_content or "Steamer" in text_content):
                    
                    # Parse out generic information
                    # E.g., Anthony Wayne, Lost of Vermilion, OH April 28, 1850
                    parts = text_content.split(",", 1)
                    name = parts[0].strip() if len(parts) > 0 else text_content
                    details = parts[1].strip() if len(parts) > 1 else ""
                    
                    wrecks.append({
                        "name": name,
                        "details": details,
                        "lat": None,
                        "lon": None,
                        "depth": "Unknown",
                        "source": "eriewrecks.com",
                        "url_reference": url
                    })
        except Exception as e:
            print(f"Error scraping {page}: {e}")

    # Load existing to append if available
    final_wrecks = []
    if os.path.exists(OUTPUT_FILE):
        with open(OUTPUT_FILE, "r") as f:
            try:
                final_wrecks = json.load(f)
            except json.JSONDecodeError:
                pass
                
    # Add new wrecks
    existing_names = {w["name"] for w in final_wrecks}
    added_count = 0
    for w in wrecks:
        if w["name"] and w["name"] not in existing_names:
            final_wrecks.append(w)
            existing_names.add(w["name"])
            added_count += 1
            
    with open(OUTPUT_FILE, "w") as f:
        json.dump(final_wrecks, f, indent=4)
        
    print(f"Found {added_count} new wrecks from eriewrecks.com!")
    print(f"Saved to {OUTPUT_FILE}")

if __name__ == "__main__":
    fetch_erie_wrecks()