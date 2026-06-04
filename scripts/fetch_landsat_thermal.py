#!/usr/bin/env python3
"""Fetch Landsat C2 L2 thermal (ST_B10 / lwir11) COGs via Microsoft Planetary
Computer (free, no auth — uses the public SAS-sign endpoint).

The thermal/material family for the triple-lock. Names each file to co-locate
with a Sentinel-2 scene by DATE when --match-prefix is given:
  <PREFIX>_<YYYYMMDD>_0_L2A.lwir11.tif  (e.g. S2A_16TFR_20240928_0_L2A.lwir11.tif)
else <landsat_id>.lwir11.tif.

ST_B10 is surface temperature DN: Kelvin = DN*0.00341802 + 149.0. The Rust
thermal concept auto-detects DN vs Kelvin (z-score is scale-invariant), so we
download the DN COG as-is.

Usage:
  python fetch_landsat_thermal.py --bbox 45.7 -84.95 45.92 -84.55 \
     --start 2024-09-01 --end 2024-11-15 --out <dir> [--max 8] [--max-cloud 30]
     [--match-prefix S2A_16TFR]
"""
import argparse, json, os, urllib.request

PC_STAC = "https://planetarycomputer.microsoft.com/api/stac/v1/search"
PC_SIGN = "https://planetarycomputer.microsoft.com/api/sas/v1/sign"

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
    with urllib.request.urlopen(url, timeout=180) as r, open(tmp, "wb") as f:
        while True:
            b = r.read(1 << 20)
            if not b: break
            f.write(b)
    os.replace(tmp, dest)

def main():
    import urllib.parse  # noqa
    ap = argparse.ArgumentParser()
    ap.add_argument("--bbox", nargs=4, type=float, required=True)
    ap.add_argument("--start", required=True)
    ap.add_argument("--end", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--max", type=int, default=8)
    ap.add_argument("--max-cloud", type=float, default=30.0)
    ap.add_argument("--match-prefix", default=None)
    args = ap.parse_args()

    la_min, lo_min, la_max, lo_max = args.bbox
    body = {
        "collections": ["landsat-c2-l2"],
        "bbox": [lo_min, la_min, lo_max, la_max],
        "datetime": f"{args.start}T00:00:00Z/{args.end}T23:59:59Z",
        "query": {"eo:cloud_cover": {"lt": args.max_cloud},
                  "platform": {"in": ["landsat-8", "landsat-9"]}},
        "limit": max(args.max * 3, 20),
    }
    feats = post(PC_STAC, body).get("features", [])
    feats.sort(key=lambda f: f["properties"].get("eo:cloud_cover", 100))
    os.makedirs(args.out, exist_ok=True)
    got = 0
    for f in feats:
        if got >= args.max:
            break
        a = f.get("assets", {}).get("lwir11")
        if not a or not a.get("href"):
            continue
        date = f["properties"]["datetime"][:10].replace("-", "")
        fname = (f"{args.match_prefix}_{date}_0_L2A.lwir11.tif"
                 if args.match_prefix else f"{f['id']}.lwir11.tif")
        dest = os.path.join(args.out, fname)
        if os.path.exists(dest) and os.path.getsize(dest) > 500_000:
            print(f"[skip] {fname}"); got += 1; continue
        try:
            signed = sign(a["href"])
            download(signed, dest)
            mb = os.path.getsize(dest) / 1e6
            print(f"[ok] {fname} {mb:.1f}MB cloud={f['properties'].get('eo:cloud_cover')}")
            got += 1
        except Exception as e:
            print(f"[err] {f['id']}: {e}")
    print(f"\nfetched {got} Landsat thermal scenes -> {args.out}")

if __name__ == "__main__":
    main()
