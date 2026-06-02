#!/usr/bin/env python3
"""
Production Satellite Downloader
Unified multisensor package runner with module toggles and one JSON report.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple

REPO = Path("/codebase/repos/wreckhunter2000-1")
DOWNLOADER = REPO / "universal_downloader.py"
SCAN_CLI = Path("/mnt/data-external/cesarops/repo/scan_cli.py")

# Keep presets configurable; not tied to name of this package.
AREA_BBOX = {
    "lake_michigan_south": (41.6, -87.3, 42.9, -86.0),
    "lake_michigan_north": (43.2, -87.5, 45.0, -86.0),
}

BUOY_SITES = {
    "45002": "https://www.ndbc.noaa.gov/data/realtime2/45002.txt",
    "45007": "https://www.ndbc.noaa.gov/data/realtime2/45007.txt",
    "45029": "https://www.ndbc.noaa.gov/data/realtime2/45029.txt",
}

MAG_SITES = {
    "USGS Geomag Data": "https://geomag.usgs.gov/ws/data/",
    "NOAA Geomag Calculator": "https://www.ngdc.noaa.gov/geomag-web/",
    "EMAG2 Global Magnetics": "https://www.ngdc.noaa.gov/geomag/emag2.html",
}


@dataclass
class StepResult:
    name: str
    command: str
    status: str
    exit_code: int
    elapsed_s: float
    total_files: Optional[int] = None
    total_size_mb: Optional[float] = None
    notes: str = ""


def load_known_credentials() -> Dict[str, str]:
    env = dict(os.environ)
    cred_files = [
        REPO / "scripts" / "credentials.sh",
        REPO / ".env",
        Path("/data/cesarops/repo/.env"),
        Path("/mnt/data-external/cesarops/repo/.env"),
    ]
    for path in cred_files:
        if not path.exists():
            continue
        for line in path.read_text(errors="ignore").splitlines():
            raw = line.strip()
            if not raw or raw.startswith("#") or "=" not in raw:
                continue
            if raw.startswith("export "):
                raw = raw[len("export ") :]
            k, v = raw.split("=", 1)
            k = k.strip()
            v = v.strip().strip('"').strip("'")
            if k and v and k not in env:
                env[k] = v
    return env


def has_earthdata(env: Dict[str, str]) -> bool:
    return bool(
        env.get("EARTHDATA_TOKEN")
        or (
            env.get("NASA_EARTHDATA_USERNAME")
            and env.get("NASA_EARTHDATA_PASSWORD")
        )
    )


def has_usgs(env: Dict[str, str]) -> bool:
    return bool(env.get("USGS_API_KEY"))


def has_copernicus(env: Dict[str, str]) -> bool:
    user = env.get("COPERNICUS_USER") or env.get("COPERNICUS_USERNAME")
    pw = env.get("COPERNICUS_PASS") or env.get("COPERNICUS_PASSWORD")
    return bool(user and pw)


def run_cmd(
    cmd: List[str], env: Dict[str, str], timeout_s: int = 1800, cwd: Path = REPO
) -> Tuple[int, str, str, float]:
    started = time.time()
    proc = subprocess.run(
        cmd,
        cwd=str(cwd),
        env=env,
        capture_output=True,
        text=True,
        timeout=timeout_s,
    )
    return proc.returncode, proc.stdout, proc.stderr, time.time() - started


def parse_totals(text: str) -> Tuple[Optional[int], Optional[float]]:
    files = None
    size = None
    m_files = re.search(r"Total files:\s*(\d+)", text)
    if m_files:
        files = int(m_files.group(1))
    m_size = re.search(r"Total size:\s*([0-9.]+)\s*MB", text)
    if m_size:
        size = float(m_size.group(1))
    return files, size


def pick_dates_from_weather(area: str, fallback: Tuple[str, str]) -> Tuple[str, str, str]:
    bbox = AREA_BBOX.get(area)
    if not bbox:
        return fallback[0], fallback[1], "fallback_no_bbox"
    lat_min, lon_min, lat_max, lon_max = bbox
    py = (
        "import sys, json; "
        "sys.path.insert(0, '/codebase/repos/wreckhunter2000-1'); "
        "from weather_service import get_scan_windows; "
        f"w=get_scan_windows({(lat_min+lat_max)/2.0}, {(lon_min+lon_max)/2.0}, '2026-05-01', '2026-05-31'); "
        "d=(w.get('post_storm_1',[])+w.get('post_storm_2',[])+w.get('post_storm_3',[])); "
        "d=sorted(set(d)); "
        "print(json.dumps({'dates': d[-2:] if len(d)>=2 else d}))"
    )
    try:
        out = subprocess.check_output(["python3", "-c", py], text=True, timeout=25)
        payload = json.loads(out)
        dates = payload.get("dates", [])
        if len(dates) >= 2:
            return dates[0], dates[-1], "weather_post_storm"
    except Exception:
        pass
    return fallback[0], fallback[1], "fallback_static"


def make_downloader_cmd(
    area: str, start: str, end: str, sensors: str, max_results: int, output: Path
) -> List[str]:
    return [
        "python3",
        str(DOWNLOADER),
        "--area",
        area,
        "--dates",
        start,
        end,
        "--sensors",
        sensors,
        "--max-results",
        str(max_results),
        "--output",
        str(output),
    ]


def run_step(name: str, cmd: List[str], env: Dict[str, str], dry_run: bool) -> StepResult:
    if dry_run:
        return StepResult(name, shlex.join(cmd), "DRY_RUN", 0, 0.0, notes="not_executed")
    code, out, err, elapsed = run_cmd(cmd, env)
    files, size = parse_totals(out)
    status = "PASS" if code == 0 else "FAIL"
    notes = err.strip()[:800] if code != 0 and err.strip() else ""
    return StepResult(
        name=name,
        command=shlex.join(cmd),
        status=status,
        exit_code=code,
        elapsed_s=round(elapsed, 2),
        total_files=files,
        total_size_mb=size,
        notes=notes,
    )


def fetch_text(url: str, timeout_s: int = 8) -> Tuple[bool, str]:
    req = urllib.request.Request(url, headers={"User-Agent": "production-satellite-downloader/1.0"})
    try:
        with urllib.request.urlopen(req, timeout=timeout_s) as r:
            text = r.read(5120).decode("utf-8", errors="ignore")
            return True, text
    except (urllib.error.URLError, TimeoutError, ValueError) as e:
        return False, str(e)


def collect_buoy_data(dry_run: bool) -> List[StepResult]:
    steps: List[StepResult] = []
    for station, url in BUOY_SITES.items():
        if dry_run:
            steps.append(StepResult(f"buoy:{station}", f"GET {url}", "DRY_RUN", 0, 0.0))
            continue
        started = time.time()
        ok, payload = fetch_text(url)
        steps.append(
            StepResult(
                name=f"buoy:{station}",
                command=f"GET {url}",
                status="PASS" if ok else "FAIL",
                exit_code=0 if ok else 1,
                elapsed_s=round(time.time() - started, 2),
                notes=(payload[:240] if ok else payload[:240]),
            )
        )
    return steps


def collect_mag_sources(dry_run: bool) -> List[StepResult]:
    steps: List[StepResult] = []
    for name, url in MAG_SITES.items():
        if dry_run:
            steps.append(StepResult(f"mag:{name}", f"HEAD/GET {url}", "DRY_RUN", 0, 0.0))
            continue
        started = time.time()
        ok, payload = fetch_text(url)
        steps.append(
            StepResult(
                name=f"mag:{name}",
                command=f"GET {url}",
                status="PASS" if ok else "WARN",
                exit_code=0 if ok else 0,
                elapsed_s=round(time.time() - started, 2),
                notes=(payload[:240] if ok else f"unreachable: {payload[:180]}"),
            )
        )
    return steps


def run_candidate_scan(start: str, end: str, dry_run: bool) -> StepResult:
    cmd = [
        "python3",
        str(SCAN_CLI),
        "--lake",
        "michigan",
        "--dates",
        start,
        end,
        "--download",
        "--label",
        "production_satellite_scan",
        "--output",
        "/tmp/production_satellite_scan",
    ]
    if dry_run:
        cmd.append("--dry-run")
    return run_step("candidate_scan", cmd, os.environ.copy(), dry_run)


def main() -> int:
    parser = argparse.ArgumentParser(description="Production Satellite Downloader")
    parser.add_argument(
        "--areas",
        default="lake_michigan_south,lake_michigan_north",
        help="Comma-separated preset areas",
    )
    parser.add_argument("--dates", nargs=2, default=["2025-01-01", "2025-01-03"])
    parser.add_argument("--max-results", type=int, default=2)
    parser.add_argument(
        "--modules",
        default="weather,downloads,candidates,buoy,mag",
        help="Comma-separated modules: weather,downloads,candidates,buoy,mag",
    )
    parser.add_argument(
        "--output-root",
        default=str(REPO / "downloads" / "production_satellite_downloader"),
    )
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    modules = {m.strip() for m in args.modules.split(",") if m.strip()}
    env = load_known_credentials()
    areas = [a.strip() for a in args.areas.split(",") if a.strip()]
    out_root = Path(args.output_root)
    out_root.mkdir(parents=True, exist_ok=True)

    profile = {
        "earthdata": has_earthdata(env),
        "usgs": has_usgs(env),
        "copernicus": has_copernicus(env),
    }

    steps: List[StepResult] = []
    selected_dates: Dict[str, Tuple[str, str, str]] = {}

    for area in areas:
        if "weather" in modules:
            start, end, src = pick_dates_from_weather(area, (args.dates[0], args.dates[1]))
        else:
            start, end, src = args.dates[0], args.dates[1], "weather_disabled"
        selected_dates[area] = (start, end, src)
        steps.append(
            StepResult(
                name=f"{area}:date_selection",
                command=f"{start}..{end}",
                status="PASS",
                exit_code=0,
                elapsed_s=0.0,
                notes=src,
            )
        )

    if "downloads" in modules:
        for area in areas:
            start, end, _ = selected_dates[area]
            area_root = out_root / area
            area_root.mkdir(parents=True, exist_ok=True)
            steps.append(
                run_step(
                    f"{area}:aws_full_spectrum",
                    make_downloader_cmd(
                        area,
                        start,
                        end,
                        "sentinel2_aws,landsat_aws,modis",
                        args.max_results,
                        area_root,
                    ),
                    env,
                    args.dry_run,
                )
            )
            if profile["earthdata"]:
                steps.append(
                    run_step(
                        f"{area}:sar_asf",
                        make_downloader_cmd(area, start, end, "sar", args.max_results, area_root),
                        env,
                        args.dry_run,
                    )
                )
                steps.append(
                    run_step(
                        f"{area}:swot_icesat2",
                        make_downloader_cmd(area, start, end, "swot,icesat2", args.max_results, area_root),
                        env,
                        args.dry_run,
                    )
                )
            else:
                steps.append(
                    StepResult(
                        f"{area}:earthdata_blocked",
                        "EARTHDATA credentials missing",
                        "SKIP",
                        0,
                        0.0,
                        notes="missing EARTHDATA token or username/password",
                    )
                )

            if profile["usgs"]:
                steps.append(
                    run_step(
                        f"{area}:usgs",
                        make_downloader_cmd(area, start, end, "usgs", args.max_results, area_root),
                        env,
                        args.dry_run,
                    )
                )
            else:
                steps.append(
                    StepResult(f"{area}:usgs_blocked", "USGS_API_KEY missing", "SKIP", 0, 0.0)
                )

    if "buoy" in modules:
        steps.extend(collect_buoy_data(args.dry_run))
    if "mag" in modules:
        steps.extend(collect_mag_sources(args.dry_run))
    if "candidates" in modules:
        # Scan over user-provided dates to keep predictable candidate window.
        steps.append(run_candidate_scan(args.dates[0], args.dates[1], args.dry_run))

    report = {
        "name": "production_satellite_downloader",
        "ts": int(time.time()),
        "areas": areas,
        "credential_profile": profile,
        "modules": sorted(modules),
        "steps": [asdict(s) for s in steps],
        "summary": {
            "pass": sum(1 for s in steps if s.status == "PASS"),
            "fail": sum(1 for s in steps if s.status == "FAIL"),
            "warn": sum(1 for s in steps if s.status == "WARN"),
            "skip": sum(1 for s in steps if s.status == "SKIP"),
            "dry_run": sum(1 for s in steps if s.status == "DRY_RUN"),
        },
    }
    out = out_root / f"production_satellite_downloader_{int(time.time())}.json"
    out.write_text(json.dumps(report, indent=2))
    print(json.dumps({"report": str(out), "summary": report["summary"]}, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())

