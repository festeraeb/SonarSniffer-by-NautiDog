#!/usr/bin/env python3
"""Search AWS STAC for Landsat 8 tiles over Kelley's Island, Sept-Oct 2015"""
import json
import urllib.request

payload = json.dumps({
    "collections": ["landsat-c2-l2"],
    "bbox": [-82.8, 41.5, -82.4, 41.8],
    "datetime": "2015-09-15T00:00:00Z/2015-10-15T00:00:00Z",
    "limit": 20
}).encode()

req = urllib.request.Request(
    "https://earth-search.aws.element84.com/v1/search",
    data=payload,
    headers={"Content-Type": "application/json"}
)
resp = json.loads(urllib.request.urlopen(req, timeout=15).read())
features = resp.get("features", [])
print(f"Found {len(features)} Landsat scenes for Argo area (Sept 15 - Oct 15, 2015)")

for f in features:
    props = f.get("properties", {})
    assets = f.get("assets", {})
    bands = [k for k in assets.keys() if not k.startswith("_")]
    print(f"\n  {f.get('id', '?')}")
    print(f"    Date: {props.get('datetime', '?')}")
    print(f"    Cloud: {props.get('eo:cloud_cover', '?')}%")
    print(f"    Bands: {[k for k in bands if 'swir' in k or 'nir' in k or 'red' in k or 'blue' in k or 'green' in k or 'thermal' in k]}")
    # Print SWIR URL if available
    for key in ['swir16', 'swir22', 'lwir11']:
        if key in assets:
            print(f"    {key}: {assets[key].get('href', '?')[:100]}")
