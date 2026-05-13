"""Check tile WGS84 extents vs known wreck locations."""
import json
import rasterio
from rasterio.warp import transform_bounds
from pathlib import Path

ROOT = Path(__file__).parent.parent
DOWNLOADS = ROOT / "downloads"

# Load wrecks
wrecks_raw = json.load(open(ROOT / "known_wrecks.json", encoding="utf-8"))["wrecks"]
wrecks = [(k, v["lat"], v["lon"]) for k, v in wrecks_raw.items()
          if isinstance(v.get("lat"), (int, float)) and isinstance(v.get("lon"), (int, float))]
print(f"Wrecks: {len(wrecks)} — lat {min(w[1] for w in wrecks):.2f}-{max(w[1] for w in wrecks):.2f}, "
      f"lon {min(w[2] for w in wrecks):.2f}-{max(w[2] for w in wrecks):.2f}\n")

# Check each tile dir
dirs = {}
for tif in DOWNLOADS.rglob("*.tif"):
    dirs.setdefault(tif.parent, []).append(tif)

print(f"Found {len(dirs)} tile directories\n")
for d, tifs in sorted(dirs.items()):
    for t in tifs[:1]:
        try:
            with rasterio.open(t) as src:
                b84 = transform_bounds(src.crs, "EPSG:4326", *src.bounds)
            # Count wrecks in this tile
            hits = [(k, la, lo) for k, la, lo in wrecks
                    if b84[0] <= lo <= b84[2] and b84[1] <= la <= b84[3]]
            rel = str(d.relative_to(DOWNLOADS))
            print(f"{rel} ({len(tifs)} tifs)")
            print(f"  lon {b84[0]:.3f} to {b84[2]:.3f}, lat {b84[1]:.3f} to {b84[3]:.3f}")
            print(f"  Wreck hits: {len(hits)}")
            for k, la, lo in hits[:3]:
                print(f"    {k}: {la},{lo}")
        except Exception as e:
            print(f"{d.name}: ERROR {e}")
    print()
