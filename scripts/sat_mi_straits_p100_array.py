#!/usr/bin/env python3
"""
Lake Michigan → Straits satellite array test (2× P100 date spreads).

Phase A: 10-day window per P100 (non-overlapping, 20 days total coverage)
Phase B: 20-day window per P100 (non-overlapping, 40 days total coverage)

Each leg: universal_downloader (full sensor array) → optional sentinel POC →
triple-lock via POST :5580/scan with corridor tile grid.
"""
from __future__ import annotations

import base64
import json
import subprocess
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import date, timedelta
from pathlib import Path

REPO = Path("/codebase/repos/wreckhunter2000-1")
DOWNLOADER = REPO / "universal_downloader.py"
SAT_CLI = REPO / "pipelines/satellite/forge_cli.py"
FORGE = "http://127.0.0.1:9100"
DETECT = "http://127.0.0.1:5580"
OUT = Path("/tmp/sat_mi_straits_p100")
# 1×1 PNG (smoke tile for triple-lock queue)
SMOKE_B64 = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
)


@dataclass
class Leg:
    gpu: str
    label: str
    bbox: tuple[float, float, float, float]  # lat_min, lon_min, lat_max, lon_max
    start: date
    end: date


def date_range(end: date, span_days: int, offset_days: int) -> tuple[date, date]:
    """Window ending `offset_days` before `end`, spanning `span_days`."""
    e = end - timedelta(days=offset_days)
    s = e - timedelta(days=span_days)
    return s, e


def grid_tiles(bbox: tuple[float, float, float, float], step: float = 0.25) -> list[dict]:
    lat_min, lon_min, lat_max, lon_max = bbox
    tiles = []
    lat = lat_min
    while lat <= lat_max:
        lon = lon_min
        while lon <= lon_max:
            tiles.append({"lat": round(lat, 4), "lon": round(lon, 4), "image_b64": SMOKE_B64})
            lon += step
        lat += step
    return tiles


def http_json(url: str, method: str = "GET", body: dict | None = None, timeout: int = 120) -> dict:
    data = None
    headers = {"Content-Type": "application/json"}
    if body is not None:
        data = json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        return {"error": e.read().decode()[:500], "code": e.code}


def run_downloader(leg: Leg, dry_run: bool = False) -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    out_dir = OUT / f"{leg.gpu}_{leg.label}_{leg.start}_{leg.end}"
    bbox_s = ",".join(str(x) for x in leg.bbox)
    cmd = [
        sys.executable,
        str(DOWNLOADER),
        "--bbox",
        bbox_s,
        "--dates",
        leg.start.isoformat(),
        leg.end.isoformat(),
        "--sensors",
        "all",
        "--max-results",
        "25",
        "--output",
        str(out_dir),
    ]
    if dry_run:
        cmd.append("--dry-run")
    print(f"\n[download] P100 {leg.gpu} {leg.label} {leg.start}..{leg.end} bbox={bbox_s}", flush=True)
    return subprocess.call(cmd, cwd=str(REPO), timeout=600)


def run_sentinel_poc(leg: Leg) -> int:
    w, s, e, n = leg.bbox[1], leg.bbox[0], leg.bbox[3], leg.bbox[2]
    out = OUT / f"poc_{leg.gpu}_{leg.label}"
    cmd = [
        sys.executable,
        str(REPO / "pipelines/satellite/wh2k_sentinel_optical_poc.py"),
        "--concept",
        "all",
        "--date-start",
        leg.start.isoformat(),
        "--date-end",
        leg.end.isoformat(),
        "--bbox-west",
        str(w),
        "--bbox-south",
        str(s),
        "--bbox-east",
        str(e),
        "--bbox-north",
        str(n),
        "--output-dir",
        str(out),
        "--max-cloud",
        "25",
    ]
    print(f"[sentinel-poc] {leg.gpu} {leg.label}", flush=True)
    try:
        return subprocess.call(cmd, cwd=str(REPO), timeout=900)
    except subprocess.TimeoutExpired:
        print("  sentinel-poc timed out (900s)", flush=True)
        return 124


def submit_triple_lock(leg: Leg, max_tiles: int = 24) -> dict:
    tiles = grid_tiles(leg.bbox, step=0.35)[:max_tiles]
    region = f"lake_mi_straits_{leg.gpu}_{leg.label}"
    body = {"region": region, "tiles": tiles}
    print(f"[triple-lock] {region} tiles={len(tiles)} dates={leg.start}..{leg.end}", flush=True)
    return http_json(f"{DETECT}/scan", "POST", body, timeout=30)


def forge_scan_region(leg: Leg) -> dict:
    bbox_s = ",".join(str(x) for x in leg.bbox)
    days = (leg.end - leg.start).days
    body = {
        "arguments": {
            "bbox": bbox_s,
            "days": days,
            "mode": "wreck",
            "region_name": f"mi_straits_{leg.gpu}_{leg.label}",
        }
    }
    print(f"[forge scan_region] {leg.gpu} days={days}", flush=True)
    return http_json(f"{FORGE}/tool/scan_region", "POST", body, timeout=1800)


def build_legs(today: date, span_per_p100: int) -> list[Leg]:
    """Non-overlapping windows: P100-0 older, P100-1 newer."""
    legs = []
    # South / central Lake Michigan
    legs.append(
        Leg(
            "p100_0",
            f"{span_per_p100}d_south",
            (42.30, -88.50, 44.50, -86.50),
            *date_range(today, span_per_p100, span_per_p100),
        )
    )
    # North Lake Michigan + Straits of Mackinac
    legs.append(
        Leg(
            "p100_1",
            f"{span_per_p100}d_north_straits",
            (44.50, -87.50, 46.10, -84.40),
            *date_range(today, span_per_p100, 0),
        )
    )
    return legs


def main() -> int:
    import argparse

    p = argparse.ArgumentParser(description="MI→Straits dual-P100 satellite + triple-lock test")
    p.add_argument("--phase", choices=["10", "20", "both"], default="both")
    p.add_argument("--dry-run", action="store_true", help="Downloader dry-run only")
    p.add_argument("--skip-download", action="store_true")
    p.add_argument("--skip-poc", action="store_true")
    p.add_argument("--skip-triple-lock", action="store_true")
    p.add_argument("--skip-forge-scan", action="store_true")
    p.add_argument("--max-tiles", type=int, default=20)
    args = p.parse_args()

    today = date.today()
    phases = []
    if args.phase in ("10", "both"):
        phases.append((10, "phase_a_10d_each_p100"))
    if args.phase in ("20", "both"):
        phases.append((20, "phase_b_20d_each_p100"))

    report: dict = {"today": today.isoformat(), "phases": []}

    health = http_json(f"{DETECT}/health", timeout=10)
    print("detection health:", json.dumps(health, indent=2))

    for span, phase_name in phases:
        phase_rec = {"name": phase_name, "span_days_per_p100": span, "legs": []}
        legs = build_legs(today, span)
        for leg in legs:
            leg_rec = {
                "gpu": leg.gpu,
                "label": leg.label,
                "bbox": leg.bbox,
                "start": leg.start.isoformat(),
                "end": leg.end.isoformat(),
            }
            if not args.skip_download:
                leg_rec["download_rc"] = run_downloader(leg, dry_run=args.dry_run)
            if not args.skip_poc and not args.dry_run:
                leg_rec["poc_rc"] = run_sentinel_poc(leg)
            if not args.skip_triple_lock:
                leg_rec["triple_lock"] = submit_triple_lock(leg, max_tiles=args.max_tiles)
            if not args.skip_forge_scan and not args.dry_run:
                leg_rec["forge_scan_region"] = forge_scan_region(leg)
            phase_rec["legs"].append(leg_rec)
        report["phases"].append(phase_rec)

    out_path = OUT / f"report_{today.isoformat()}.json"
    OUT.mkdir(parents=True, exist_ok=True)
    if out_path.exists():
        try:
            prev = json.loads(out_path.read_text())
            prev_phases = prev.get("phases", [])
            # merge by phase name
            names = {p["name"] for p in report["phases"]}
            report["phases"] = [p for p in prev_phases if p["name"] not in names] + report["phases"]
        except json.JSONDecodeError:
            pass
    out_path.write_text(json.dumps(report, indent=2))
    print(f"\nReport: {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
