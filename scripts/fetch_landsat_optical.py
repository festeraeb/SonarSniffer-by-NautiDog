#!/usr/bin/env python3
"""Fetch Landsat C2 L2 spectral bands (blue/green/red/swir16) via Planetary Computer.

Same approach as fetch_landsat_thermal.py but pulls optical bands instead of
(or in addition to) thermal. Landsat 7/8/9 all have these; L4/5 TM has slightly
different band names but same physics. PC serves COG for all.

Usage:
  python fetch_landsat_optical.py --bbox 45.7 -84.95 45.92 -84.55 \
     --start 2014-04-01 --end 2017-11-30 --out <dir> --max 40
"""
import argparse, json, os, urllib.request, urllib.parse

PC_STAC = "https://planetarycomputer.microsoft.com/api/stac/v1/search"
PC_SIGN = "https://planetarycomputer.microsoft.com/api/sas/v1/sign"

# Landsat C2 L2 band asset keys (same across L5/7/8/9 on PC):
BANDS = ["blue", "green", "red", "swir16"]

def post(url, body):
    req = urllib.request.Request(url, data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)

def sign(href):
    req = urllib.request.Request(f"{PC_SIGN}?href={urllib.parse.quote(href, safe='')}")
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r).get("href", href)

def download(url, dest):
    tmp = dest + ".part"
    with urllib.request.urlopen(url, timeout=300) as r, open(tmp, "wb") as f:
        while True:
            b = r.read(1 << 20)
            if not b: break
            f.write(b)
    os.replace(tmp, dest)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bbox", nargs=4, type=float, required=True)
    ap.add_argument("--start", required=True)
    ap.add_argument("--end", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--max", type=int, default=40, help="max SCENES (each = 4 band files)")
    ap.add_argument("--max-cloud", type=float, default=25.0)
    args = ap.parse_args()

    la_min, lo_min, la_max, lo_max = args.bbox
    body = {
        "collections": ["landsat-c2-l2"],
        "bbox": [lo_min, la_min, lo_max, la_max],
        "datetime": f"{args.start}T00:00:00Z/{args.end}T23:59:59Z",
        "query": {"eo:cloud_cover": {"lt": args.max_cloud},
                  "platform": {"in": ["landsat-5", "landsat-7", "landsat-8", "landsat-9"]}},
        "limit": min(args.max * 3, 100),
    }
    feats = post(PC_STAC, body).get("features", [])
    feats.sort(key=lambda f: f["properties"].get("eo:cloud_cover", 100))
    os.makedirs(args.out, exist_ok=True)
    got = 0
    for f in feats:
        if got >= args.max:
            break
        assets = f.get("assets", {})
        scene_id = f["id"]
        date = f["properties"]["datetime"][:10].replace("-", "")
        any_fetched = False
        for band in BANDS:
            a = assets.get(band)
            if not a or not a.get("href"):
                continue
            fname = f"{scene_id}.{band}.tif"
            dest = os.path.join(args.out, fname)
            if os.path.exists(dest) and os.path.getsize(dest) > 500_000:
                any_fetched = True
                continue
            try:
                signed = sign(a["href"])
                download(signed, dest)
                mb = os.path.getsize(dest) / 1e6
                print(f"[ok] {fname} {mb:.1f}MB")
                any_fetched = True
            except Exception as e:
                print(f"[err] {fname}: {e}")
        if any_fetched:
            got += 1
    print(f"\nfetched {got} Landsat scenes ({got*len(BANDS)} band files) -> {args.out}")

if __name__ == "__main__":
    main()
