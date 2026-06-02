#!/usr/bin/env python3
import base64
import io
import json
import sys
from pathlib import Path

import requests

out = Path(sys.argv[1])
tpu = sys.argv[2]
jitter = sys.argv[3]

# Valid 1x1 PNG
b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="

meta = {
    "tile_id": "Holloway-probe-42N83W",
    "lat": 42.331,
    "lon": -83.048,
    "pass_id": "pass-2026-06-01",
}
tpu_r = requests.post(
    f"{tpu.rstrip('/')}/infer",
    json={"image_base64": b64, "meta": meta},
    timeout=60,
)
tpu_r.raise_for_status()
(out / "tpu_infer.json").write_text(json.dumps(tpu_r.json(), indent=2))

jit_r = requests.post(
    f"{jitter.rstrip('/')}/jitter",
    json={
        "tile_id": meta["tile_id"],
        "thermal_timeseries": ["t0", "t1", "t2", "t3"],
        "coordinates": {"lat": meta["lat"], "lon": meta["lon"]},
        "depth_estimate_m": 42.0,
    },
    timeout=60,
)
jit_r.raise_for_status()
(out / "movidius_jitter.json").write_text(json.dumps(jit_r.json(), indent=2))

tpu_j = tpu_r.json()
jit_j = jit_r.json()
packet = {
    "mission": "sunken_ship_anchor_hunt",
    "tile_id": meta["tile_id"],
    "coordinates": {"lat": meta["lat"], "lon": meta["lon"]},
    "repeat_pass_hypothesis": "glint_ripple_dark_clear_water_same_pixels",
    "tpu_scan": {
        "detection_count": len(tpu_j.get("detections") or []),
        "top_detections": (tpu_j.get("detections") or [])[:8],
        "used_tpu": tpu_j.get("used_tpu"),
        "took_s": tpu_j.get("took_s"),
    },
    "movidius_jitter": jit_j,
    "coder_task": "Rank wreck candidates; cue matrix; FP guards; sketch process_tile()",
}
(out / "accel_scan_packet.json").write_text(json.dumps(packet, indent=2))
print(json.dumps(packet, indent=2)[:500])
