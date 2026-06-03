#!/usr/bin/env python3
"""Merge Rust glint_persistence_map with TPU/Movidius accel_scan_packet for corroboration."""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path


def haversine_m(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    r = 6_371_000.0
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dp = math.radians(lat2 - lat1)
    dl = math.radians(lon2 - lon1)
    a = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * r * math.asin(math.sqrt(a))


def load_persist(path: Path) -> list[dict]:
    if not path.is_file():
        return []
    data = json.loads(path.read_text())
    if isinstance(data, dict) and "cells" in data:
        return data["cells"]
    if isinstance(data, list):
        return data
    return []


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--persist", type=Path, required=True)
    ap.add_argument("--accel", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--radius-m", type=float, default=2500.0)
    args = ap.parse_args()

    cells = load_persist(args.persist)
    accel = json.loads(args.accel.read_text()) if args.accel.is_file() else []
    if isinstance(accel, dict) and "items" in accel:
        accel = accel["items"]
    if not isinstance(accel, list):
        accel = [accel]

    fused: list[dict] = []
    for row in accel:
        coords = row.get("coordinates") or {}
        alat, alon = coords.get("lat"), coords.get("lon")
        if alat is None or alon is None:
            continue
        jit = row.get("jitter_signature") or {}
        validations = jit.get("validation") or []
        mov = next((v for v in validations if v.get("device") == "movidius_ncs2"), None)
        tpu = row.get("tpu_scan") or {}
        best_p = 0.0
        best_cell = None
        for c in cells:
            lat = c.get("lat") or c.get("latitude")
            lon = c.get("lon") or c.get("longitude")
            p = c.get("persistence") or c.get("value") or 0.0
            if lat is None or lon is None:
                continue
            if haversine_m(alat, alon, float(lat), float(lon)) <= args.radius_m:
                if float(p) > best_p:
                    best_p = float(p)
                    best_cell = c
        fused.append(
            {
                "scene_id": row.get("scene_id") or row.get("tile_id"),
                "accel_lat": alat,
                "accel_lon": alon,
                "tpu_detection_count": tpu.get("detection_count", 0),
                "movidius_agreement": (mov or {}).get("agreement"),
                "movidius_agreed": (mov or {}).get("agreed"),
                "glint_persistence_nearby": best_p,
                "glint_cell": best_cell,
                "corroborated": best_p >= 0.08 and (mov or {}).get("agreed"),
            }
        )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    packet = {"fused": fused, "sources": {"persist": str(args.persist), "accel": str(args.accel)}}
    args.out.write_text(json.dumps(packet, indent=2))
    print(json.dumps({"n_accel": len(accel), "n_fused": len(fused), "corroborated": sum(1 for x in fused if x["corroborated"])}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
