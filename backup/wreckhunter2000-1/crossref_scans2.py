import json
from pathlib import Path

SHIPPING_LANES = {
    "Lake_Superior_Whitefish_Bay": "-85.2,46.6,-84.3,46.9",
    "Straits_of_Mackinac": "-84.9,45.7,-84.1,45.9",
    "Lake_Huron_Thunder_Bay": "-83.3,44.9,-82.9,45.2",
    "Lake_Michigan_Manitou_Passage": "-86.2,44.9,-85.9,45.2",
    "Lake_Erie_Pelee_Passage": "-82.8,41.7,-82.4,41.9"
}

def box_intersect(b1, b2):
    try:
        return not (b1[2] < b2[0] or b1[0] > b2[2] or b1[3] < b2[1] or b1[1] > b2[3])
    except Exception:
        return False

wrecks = []
for db in ["known_wrecks.json", "known_wrecks_erie.json"]:
    p = Path(db)
    if p.exists():
        data = json.loads(p.read_text(encoding="utf-8"))
        if "wrecks" in data:
            for k,v in data["wrecks"].items():
                wrecks.append((k, v))
        elif type(data) is list:
            for i,v in enumerate(data):
                wrecks.append((v.get("ship_name", f"ErieWreck-{i}"), v))

print(f"Loaded {len(wrecks)} wrecks.")
for lane, bbox_str in SHIPPING_LANES.items():
    west, south, east, north = map(float, bbox_str.split(","))
    lane_bbox = (west, south, east, north)
    
    hits = []
    for w_name, w_data in wrecks:
        lat_min = w_data.get('lat_min', w_data.get('lat', 0.0))
        lat_max = w_data.get('lat_max', w_data.get('lat', 0.0))
        lon_min = w_data.get('lon_min', w_data.get('lon', 0.0))
        lon_max = w_data.get('lon_max', w_data.get('lon', 0.0))
        if lat_min == 0.0 and 'location' in w_data: 
            lat_min = w_data['location'].get('lat', 0.0)
            lat_max = w_data['location'].get('lat', 0.0)
            lon_min = w_data['location'].get('lon', 0.0)
            lon_max = w_data['location'].get('lon', 0.0)
        
        w_bbox = (lon_min, lat_min, lon_max, lat_max)
        if box_intersect(lane_bbox, w_bbox):
            hits.append(w_name)
            
    print(f"Lane {lane} hits {len(hits)} known targets/wrecks:")
    if hits:
        print("  - " + ", ".join(hits[:10]) + ("..." if len(hits)>10 else ""))
