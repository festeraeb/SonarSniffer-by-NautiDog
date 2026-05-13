#!/usr/bin/env python3
"""
Straits of Mackinac simulated scan — sovereign-cloud pipeline test.

Fires the 4-pass detection pipeline against three Mackinac sites using
calibrated Landsat 8/9 HLS surface-reflectance band data.

Pass architecture
  Pass 1  Scout       — glint / hydrocarbon / thermal anomaly detection
  Pass 2  Tiling      — decouple ROIs from UTM, build 16-square density tiles
  Pass 3  Analyst     — curvelet filtering, spectral analysis, bathymetry (stubs)
  Pass 4  Stitch      — temporal stacking over stored tiles

Band ordering: [Blue, Green, Red, NIR, SWIR1, SWIR2, Thermal]
  Bands 0-5: surface reflectance (unitless, ~0.0–0.15 over water)
  Band 6:    thermal (normalized brightness temperature proxy, ~0.10–0.20)

Scout detectors (from pipeline.rs):
  glint         = (NIR / (Blue + 0.001) − 1.0).clamp(0, 1)
  hydrocarbon   = 1.0 − (SWIR2 / (Red + 0.001)).clamp(0, 1)
  thermal_anom  = |Thermal − mean(all_bands)| / (mean + 0.001)   [clamped 0–1]

Usage:
    python scripts/test_mackinac_scan.py [--host http://localhost:8765] [--verbose]
    python scripts/test_mackinac_scan.py --host http://home.cesarops.com:8765
"""

import argparse
import json
import sys
import time
from dataclasses import dataclass
from typing import Optional

# Force UTF-8 output on Windows consoles that default to cp1252
if hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except Exception:
        pass

try:
    import requests
except ImportError:
    sys.exit("Install requests:  pip install requests")

# ── Sites ─────────────────────────────────────────────────────────────────────

@dataclass
class Site:
    name: str
    lat: float
    lon: float
    depth_ft: int
    bands: list[float]   # [Blue, Green, Red, NIR, SWIR1, SWIR2, Thermal]
    notes: str

SITES = [
    Site(
        name="SS Cedarville (wreck)",
        lat=45.7784, lon=-84.5842, depth_ft=35,
        bands=[0.082, 0.078, 0.042, 0.118, 0.009, 0.004, 0.148],
        notes=(
            "Bulk limestone/cement carrier sunk 1965 after collision with MV Topdalsfjord. "
            "35 ft depth. Fuel oil cargo remnants → elevated SWIR2 suppression. "
            "Shallow steel hull → elevated NIR reflectance."
        ),
    ),
    Site(
        name="SW Moreland (wreck)",
        lat=45.7820, lon=-84.6100, depth_ft=42,
        bands=[0.071, 0.065, 0.038, 0.052, 0.008, 0.006, 0.138],
        notes=(
            "Self-unloading bulk carrier, 1959. 42 ft depth. "
            "Moderate NIR (slightly deeper), residual petroleum signatures."
        ),
    ),
    Site(
        name="Martin Channel open water (baseline)",
        lat=45.7710, lon=-84.5600, depth_ft=90,
        bands=[0.062, 0.055, 0.031, 0.014, 0.005, 0.005, 0.112],
        notes=(
            "Open channel — no known wreck. 90 ft depth. "
            "Low NIR (deep clear water absorbs near-infrared), "
            "SWIR ≈ SWIR2 (no hydrocarbon suppression)."
        ),
    ),
]

# ── Scout signal preview (mirrors pipeline.rs detector logic) ─────────────────

def preview_scout(bands: list[float]) -> dict:
    glint = max(0.0, min(1.0, bands[3] / (bands[0] + 0.001) - 1.0))
    hydrocarbon = max(0.0, min(1.0, 1.0 - bands[5] / (bands[2] + 0.001)))
    mean = sum(bands) / len(bands)
    thermal = min(1.0, abs(bands[-1] - mean) / (mean + 0.001))
    return {"glint": glint, "hydrocarbon": hydrocarbon, "thermal": thermal,
            "max_confidence": max(glint, hydrocarbon, thermal)}

# ── HTTP helpers ──────────────────────────────────────────────────────────────

def health_check(host: str) -> bool:
    try:
        r = requests.get(f"{host}/health", timeout=4)
        return r.status_code == 200
    except Exception:
        return False

def run_pipeline(host: str, site: Site) -> Optional[dict]:
    payload = {"lat": site.lat, "lon": site.lon, "bands": site.bands}
    try:
        r = requests.post(
            f"{host}/v1/pipeline/run",
            json=payload,
            timeout=30,
        )
        r.raise_for_status()
        return r.json()
    except requests.exceptions.ConnectionError:
        return None
    except requests.exceptions.HTTPError as e:
        print(f"  [HTTP {e.response.status_code}] {e.response.text[:200]}")
        return None

# ── Report formatting ─────────────────────────────────────────────────────────

BAR = "█"
EMPTY = "░"

def confidence_bar(v: float, width: int = 20) -> str:
    filled = int(v * width)
    color = ""
    reset = ""
    try:
        if sys.stdout.isatty():
            if v >= 0.65:
                color = "\033[31m"    # red
            elif v >= 0.1:
                color = "\033[33m"    # yellow
            else:
                color = "\033[32m"    # green
            reset = "\033[0m"
    except Exception:
        pass
    bar = BAR * filled + EMPTY * (width - filled)
    return f"{color}[{bar}]{reset} {v:.3f}"

def print_site_header(site: Site) -> None:
    print()
    print(f"  ╔{'═' * 62}╗")
    print(f"  ║  {site.name:<60}║")
    print(f"  ║  {site.lat:.4f}°N  {abs(site.lon):.4f}°W   depth: {site.depth_ft} ft{' ' * (24 - len(str(site.depth_ft)))}║")
    print(f"  ╚{'═' * 62}╝")

def print_pass(result: dict, idx: int) -> None:
    name = result.get("pass", "?")
    conf = result.get("anomaly_confidence", 0.0)
    ms   = result.get("elapsed_ms", 0)
    ok   = "✓" if result.get("success") else "✗"
    bar  = confidence_bar(conf)
    print(f"  Pass {idx+1} [{name:<18}] {ok}  conf: {bar}  ({ms}ms)")

def print_scout_detail(out: dict) -> None:
    g = out.get("glint", 0.0)
    h = out.get("hydrocarbon", 0.0)
    t = out.get("thermal", 0.0)
    print(f"           glint:          {confidence_bar(g, 12)}")
    print(f"           hydrocarbon:    {confidence_bar(h, 12)}")
    print(f"           thermal_anom:   {confidence_bar(t, 12)}")

def print_analyst_detail(out: dict) -> None:
    c = out.get("curvelet_score", 0.0)
    s = out.get("spectral_score", 0.0)
    b = out.get("bathymetry_score", 0.0)
    fc = out.get("final_confidence", 0.0)
    print(f"           curvelet:       {confidence_bar(c, 12)}")
    print(f"           spectral:       {confidence_bar(s, 12)}")
    print(f"           bathymetry:     {confidence_bar(b, 12)}")
    print(f"           final:          {confidence_bar(fc, 12)}")

# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Mackinac pipeline scan test")
    parser.add_argument("--host", default="http://localhost:8765",
                        help="sovereign-cloud API base URL")
    parser.add_argument("--verbose", action="store_true",
                        help="Print full JSON responses")
    args = parser.parse_args()

    host = args.host.rstrip("/")

    print()
    print("═" * 66)
    print("  WRECKHUNTER 2000 — Straits of Mackinac Scan")
    print("  sovereign-cloud pipeline test")
    print(f"  target node: {host}")
    print("═" * 66)

    # ── Health check ──────────────────────────────────────────────────────────
    print()
    print("  Checking sovereign-cloud health...")
    alive = health_check(host)
    if alive:
        print("  ✓ sovereign-cloud is up")
    else:
        print(f"  ✗ sovereign-cloud not reachable at {host}")
        print()
        print("  Start sovereign-cloud with:")
        print("    set OLLAMA_BASE_URL=http://localhost:11434/v1")
        print("    sovereign-cloud.exe")
        print()
        print("  Falling back to local signal preview (no pipeline execution).")
        _local_preview()
        return

    # ── Run scan ──────────────────────────────────────────────────────────────
    print()
    print("  Firing 4-pass pipeline for each site...")

    site_results = []
    total_start = time.time()

    for site in SITES:
        print_site_header(site)
        print(f"  notes: {site.notes}")
        print()

        # Local signal preview (before network call)
        preview = preview_scout(site.bands)
        print(f"  Local scout preview — max confidence: {preview['max_confidence']:.3f}")
        if preview['max_confidence'] < 0.1:
            print("  [predicted: below 0.10 threshold — pipeline will stop after Pass 1]")
        else:
            print("  [predicted: threshold cleared — full 4-pass run expected]")

        print()
        t0 = time.time()
        results = run_pipeline(host, site)
        elapsed = time.time() - t0

        if results is None:
            print("  [pipeline call failed — is sovereign-cloud running?]")
            site_results.append((site, None))
            continue

        # results is a list of PassResults
        if not isinstance(results, list):
            results = [results]

        for i, r in enumerate(results):
            print_pass(r, i)
            if args.verbose or r.get("pass") == "scout":
                out = r.get("output", {})
                if r.get("pass") == "scout":
                    print_scout_detail(out)
            if r.get("pass") == "analyst":
                print_analyst_detail(r.get("output", {}))

        passes_run = len(results)
        final_conf = results[-1].get("anomaly_confidence", 0.0) if results else 0.0
        alert = any(r.get("anomaly_confidence", 0.0) > 0.65 for r in results)
        print()
        print(f"  Passes completed: {passes_run}/4   total wall time: {elapsed:.2f}s")
        if alert:
            print("  ⚠ ANOMALY ALERT triggered (confidence > 0.65)")
        else:
            print("  ✓ No alert threshold crossed")

        if args.verbose:
            print()
            print("  Full response:")
            print(json.dumps(results, indent=2))

        site_results.append((site, results))

    total_elapsed = time.time() - total_start

    # ── Summary table ─────────────────────────────────────────────────────────
    print()
    print("═" * 66)
    print("  SCAN SUMMARY")
    print("═" * 66)
    print(f"  {'Site':<38}  {'Passes':>6}  {'Max Conf':>9}  Alert")
    print(f"  {'-'*38}  {'-'*6}  {'-'*9}  -----")
    for site, results in site_results:
        if results is None:
            print(f"  {site.name:<38}  {'ERR':>6}  {'---':>9}  ---")
            continue
        passes = len(results)
        max_conf = max((r.get("anomaly_confidence", 0.0) for r in results), default=0.0)
        alerted = any(r.get("anomaly_confidence", 0.0) > 0.65 for r in results)
        alert_str = "⚠ YES" if alerted else "no"
        print(f"  {site.name:<38}  {passes:>6}  {max_conf:>9.3f}  {alert_str}")

    print()
    print(f"  Total scan time: {total_elapsed:.2f}s  |  3 sites  |  up to 12 passes")
    print()
    print("  Note: Analyst pass uses stub math (0.3 / 0.3 / 0.2 → conf 0.267).")
    print("  Replace curvelet_filter_stub / spectral_analysis_stub / bathymetry_stub")
    print("  in sovereign-cloud/src/pipeline.rs with real wgpu compute dispatch.")
    print()


def _local_preview():
    """Offline signal preview when sovereign-cloud is unavailable."""
    print("  Local signal preview (no pipeline execution):")
    print()
    for site in SITES:
        print(f"  {site.name}")
        p = preview_scout(site.bands)
        print(f"    bands: {site.bands}")
        print(f"    glint:        {p['glint']:.4f}")
        print(f"    hydrocarbon:  {p['hydrocarbon']:.4f}")
        print(f"    thermal:      {p['thermal']:.4f}")
        print(f"    max:          {p['max_confidence']:.4f}")
        passes = ">= 0.10 → all 4 passes" if p['max_confidence'] >= 0.1 else "< 0.10 → scout only"
        print(f"    → {passes}")
        print()


if __name__ == "__main__":
    main()
