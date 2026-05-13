#!/usr/bin/env python3
"""ogsrl_well_discriminator.py
─────────────────────────────────────────────────────────────────────────────
Standalone OGSRL well-based mag anomaly discriminator.

Downloads the Ontario Oil, Gas & Salt Resources Library public well CSV,
then cross-references detected mag anomaly candidates against known
well / pipeline locations to flag:

  1. Proximity hits      — candidate within PROX_RADIUS_M of a known well
  2. Side-pull hits      — anomaly elongation axis points toward nearest well
  3. Orientation hits    — long axis aligns with Ontario lot grid (E-W / N-S)
                           rather than shipping lane headings or natural geology

OGSRL source (free public CSV, updated periodically):
    https://www.ogsrlibrary.com/data/petroleum_well_data_ontario_csv.zip

Background
──────────
At aeromagnetic survey resolution (≥200 m grid) a vertical gas well casing
creates a broad circular anomaly that is indistinguishable from a small wreck
by amplitude alone.  Two geometric signatures betray wells and pipelines:

  Side pull  ─ the detection centroid is offset toward a known well because
    the dipole anomaly is peaked at the survey altitude directly above the well
    but the nearest resolution cell is shifted along the flight line.  The
    anomaly's elongation axis therefore points *toward* the well rather than
    along a vessel heading.

  Grid orientation  ─ Ontario surveys were laid out on a lot/concession grid
    (E-W / N-S cardinal axes).  Pipelines connecting well clusters follow this
    grid.  An elongated anomaly aligned to 0°/90°/180°/270° inside a well
    field is almost certainly a pipeline, not a wreck.

Lake Erie geomagnetic reference (IGRF-12, epoch 2015.75, ~42°N / 82°W):
    Declination  : -10.2°  (magnetic N is 10.2° west of true N)
    Inclination  : +68.1°  (steep downward)
    Total field  :  56 200 nT
    Horizontal   :  21 000 nT

Usage:
    # Fetch OGSRL wells from web (first run) and discriminate MB2 candidates
    python ogsrl_well_discriminator.py --candidates mb2_candidates.json --fetch

    # Reuse cached wells
    python ogsrl_well_discriminator.py --candidates scan_output.json

    # Point at a pre-downloaded OGSRL CSV (skip web)
    python ogsrl_well_discriminator.py --csv ./ogsrl.csv --candidates hc.json

    # Adjust proximity radius
    python ogsrl_well_discriminator.py --candidates hc.json --prox-radius 3000

Outputs:
    <stem>_ogsrl_discriminated.json   — full annotated results
    <stem>_ogsrl_summary.txt          — human-readable summary
"""

from __future__ import annotations

import argparse
import csv
import io
import json
import logging
import math
import os
import sys
import zipfile
from collections import Counter
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Optional
from urllib.request import Request, urlopen

# ─────────────────────────────────────────────────────────────────────────────
# Constants
# ─────────────────────────────────────────────────────────────────────────────

OGSRL_CSV_ZIP_URL = (
    "https://www.ogsrlibrary.com/data/petroleum_well_data_ontario_csv.zip"
)
DEFAULT_CACHE = Path("data/ogsrl_wells_cache.json")

# Lake Erie bounding box — generous, matches the rest of the pipeline
LAKE_ERIE_BBOX = {
    "lat_min": 41.20,
    "lat_max": 42.95,
    "lon_min": -83.60,
    "lon_max": -78.70,
}

# ── IGRF-12 reference, Lake Erie central basin, epoch 2015.75 ───────────────
# Source: NOAA IGRF calculator at 42°N, 82°W, 0 km alt
IGRF_2015_ERIE = {
    "declination_deg": -10.2,   # West: magnetic N is 10.2° west of true N
    "inclination_deg": +68.1,   # Steep downward (high northern latitude)
    "total_field_nT": 56_200,
    "horizontal_nT":  21_000,
    "epoch": 2015.75,
}

# The geomagnetic field's horizontal vector points toward magnetic north, which
# is ~-10° (nearly N-S).  Natural geological lineaments tend to align with the
# regional field strike.
GEOMAG_STRIKE_DEG: float = IGRF_2015_ERIE["declination_deg"] % 180  # 169.8 ≈ 170°

# Ontario Lands Division grid: lot/concession surveys are cardinal, so pipelines
# connecting well clusters are strongly E-W or N-S.
ONTARIO_GRID_BEARINGS = [0.0, 90.0, 180.0, 270.0]

# Erie shipping lane heading: Cleveland ↔ Buffalo corridor ≈ ENE/WSW ≈ 075°/255°
SHIPPING_LANE_BEARING = 75.0

# ── Classification thresholds ────────────────────────────────────────────────
PROX_RADIUS_M         = 2_000.0   # m — anomaly within this distance → flag
SIDE_PULL_CONE_DEG    = 35.0      # ° — long-axis aligned to well within this tolerance
PIPELINE_ASPECT_RATIO = 2.5       # ratio — width:height > this → suspect elongated source
GEOMAG_TOL_DEG        = 30.0      # ° — tolerance for natural geomagnetic strike alignment
GRID_TOL_DEG          = 25.0      # ° — tolerance for Ontario lot-grid alignment
LANE_TOL_DEG          = 30.0      # ° — tolerance for shipping lane alignment

# ─────────────────────────────────────────────────────────────────────────────
# Data structures
# ─────────────────────────────────────────────────────────────────────────────

@dataclass
class OGSRLWell:
    well_id:    str
    name:       str
    lat:        float
    lon:        float
    status:     str
    well_class: str   # GAS / OIL / SALT / WATER / UNKNOWN
    well_type:  str   # VERTICAL / HORIZONTAL / DIRECTIONAL / etc.
    township:   str
    county:     str
    target:     str
    is_lake_erie: bool = False


@dataclass
class DiscriminatorResult:
    # ── Input candidate fields ──────────────────────────────────────────────
    label_id:           int   = 0
    center_lat:         float = 0.0
    center_lon:         float = 0.0
    composite_score:    float = 0.0
    dipole_score:       float = 0.0
    tier:               str   = ""
    width_m:            float = 0.0
    height_m:           float = 0.0
    amplitude_peak_nT:  float = 0.0
    z_score:            float = 0.0

    # ── Nearest well ────────────────────────────────────────────────────────
    nearest_well_id:    str   = ""
    nearest_well_name:  str   = ""
    nearest_well_class: str   = ""
    nearest_well_lat:   float = 0.0
    nearest_well_lon:   float = 0.0
    nearest_well_m:     float = 99_999.0
    within_prox:        bool  = False

    # ── Side-pull analysis ──────────────────────────────────────────────────
    anomaly_long_axis_bearing:  float = 0.0   # cardinal direction of long axis
    bearing_to_nearest_well:    float = 0.0   # bearing from candidate → well
    side_pull_angular_diff:     float = 180.0 # angular diff (0-90°, lower = more aligned)
    side_pull_flag:             bool  = False

    # ── Orientation analysis ────────────────────────────────────────────────
    aspect_ratio:               float = 1.0
    is_pipeline_elongation:     bool  = False
    geomag_alignment_deg:       float = 90.0  # deviation from geomagnetic strike
    grid_alignment_deg:         float = 90.0  # deviation from nearest Ontario grid axis
    lane_alignment_deg:         float = 90.0  # deviation from Erie shipping lane
    orientation_flag:           str   = ""    # grid_pipeline / shipping_lane / geomagnetic / unknown

    # ── Final output ────────────────────────────────────────────────────────
    well_score:     float       = 0.0   # 0.0–1.0 likelihood this is a well/pipeline artifact
    classification: str         = "unknown"  # wreck / well_pull / pipeline / suspicious / unknown
    reasons:        list[str]   = field(default_factory=list)


# ─────────────────────────────────────────────────────────────────────────────
# Logging
# ─────────────────────────────────────────────────────────────────────────────

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s  %(levelname)-7s  %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("ogsrl_disc")

# ─────────────────────────────────────────────────────────────────────────────
# Geometry utilities
# ─────────────────────────────────────────────────────────────────────────────

def haversine_m(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    """Great-circle distance in metres (WGS-84 mean radius)."""
    R = 6_371_000.0
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlam = math.radians(lon2 - lon1)
    a = math.sin(dphi / 2) ** 2 + math.cos(phi1) * math.cos(phi2) * math.sin(dlam / 2) ** 2
    return 2 * R * math.asin(math.sqrt(max(0.0, min(1.0, a))))


def bearing_deg(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    """Forward azimuth, degrees true (0° = N, 90° = E, 180° = S, 270° = W)."""
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dlam = math.radians(lon2 - lon1)
    y = math.sin(dlam) * math.cos(phi2)
    x = math.cos(phi1) * math.sin(phi2) - math.sin(phi1) * math.cos(phi2) * math.cos(dlam)
    return (math.degrees(math.atan2(y, x)) + 360.0) % 360.0


def angular_diff_axis(a: float, b: float) -> float:
    """Angular difference modulo 180° — for undirected *axes* (0–90°).

    An axis at 10° and one at 190° are the same axis; the difference is 0°.
    An axis at 0° and one at 90° have the maximum difference of 90°.
    """
    diff = abs(a - b) % 360.0
    if diff > 180.0:
        diff = 360.0 - diff
    if diff > 90.0:
        diff = 180.0 - diff
    return diff


def long_axis_bearing_from_bbox(width_m: float, height_m: float) -> float:
    """Infer the long axis of the anomaly footprint from its bounding-box dims.

    width_m  = E-W extent in metres
    height_m = N-S extent in metres

    Returns the bearing of the long axis (0° if N-S, 90° if E-W).
    This is a coarse proxy — the true principal axis requires the raw pixel array.
    """
    if width_m <= 0.0 or height_m <= 0.0:
        return 0.0
    return 90.0 if width_m >= height_m else 0.0


# ─────────────────────────────────────────────────────────────────────────────
# OGSRL data access
# ─────────────────────────────────────────────────────────────────────────────

def _fetch_zip_bytes(url: str, timeout: int = 90) -> bytes:
    """Download a ZIP file from *url*. Returns raw bytes."""
    log.info("Downloading OGSRL CSV ZIP from %s …", url)
    req = Request(url, headers={"User-Agent": "WreckHunter2000/1.0 (research)"})
    with urlopen(req, timeout=timeout) as resp:
        data = resp.read()
    log.info("Downloaded %.1f MB", len(data) / 1_048_576)
    return data


def _parse_ogsrl_rows(reader: csv.DictReader) -> list[OGSRLWell]:
    """Convert OGSRL CSV rows to OGSRLWell objects, filtered to Erie bbox."""
    wells: list[OGSRLWell] = []
    skipped = 0

    for row in reader:
        try:
            lat = float(row.get("SUR_LAT83", "") or 0)
            lon = float(row.get("SUR_LONG83", "") or 0)
        except (ValueError, TypeError):
            skipped += 1
            continue

        if lat == 0.0 or lon == 0.0:
            skipped += 1
            continue

        # Township field contains "LAKE ERIE" for offshore wells
        township = (row.get("TOWNSHIP", "") or "").strip()
        is_lake  = "lake erie" in township.lower()

        in_bbox = (
            LAKE_ERIE_BBOX["lat_min"] <= lat <= LAKE_ERIE_BBOX["lat_max"]
            and LAKE_ERIE_BBOX["lon_min"] <= lon <= LAKE_ERIE_BBOX["lon_max"]
        )

        if not (is_lake or in_bbox):
            continue

        # Classify by well class / type / target — OGSRL uses the CLASS column
        # with values like GAS, OIL, OGS (Oil Gas Salt), SALT, WAT, etc.
        cls_raw  = (row.get("CLASS",     "") or row.get("WELL_CLASS", "") or "").strip().upper()
        type_raw = (row.get("WELL_TYPE", "") or row.get("TYPE",       "") or "").strip().upper()
        tgt_raw  = (row.get("TARGET",    "") or "").strip().upper()

        combined = f"{cls_raw} {type_raw} {tgt_raw}"
        if "GAS" in combined:
            well_class = "GAS"
        elif "OIL" in combined:
            well_class = "OIL"
        elif "SALT" in combined:
            well_class = "SALT"
        elif "WAT" in combined or "WATER" in combined:
            well_class = "WATER"
        else:
            well_class = cls_raw if cls_raw else "UNKNOWN"

        wells.append(OGSRLWell(
            well_id    = (row.get("WELL_ID", "") or row.get("LICENCE_NO", "") or "").strip(),
            name       = (row.get("FULL_NAME", "") or row.get("WELL_NAME", "")).strip(),
            lat        = lat,
            lon        = lon,
            status     = (row.get("CUR_STATUS", "") or "").strip(),
            well_class = well_class,
            well_type  = (row.get("MODE", "") or type_raw).strip(),
            township   = township.strip(),
            county     = (row.get("COUNTY", "") or "").strip(),
            target     = tgt_raw,
            is_lake_erie = is_lake,
        ))

    log.info(
        "Parsed %d Lake Erie wells (%d skipped — no coords or outside bbox); "
        "%d offshore Lake Erie",
        len(wells), skipped, sum(1 for w in wells if w.is_lake_erie),
    )
    return wells


def fetch_ogsrl_from_web(url: str = OGSRL_CSV_ZIP_URL) -> list[OGSRLWell]:
    """Download and parse the OGSRL CSV ZIP directly from the web."""
    raw_zip = _fetch_zip_bytes(url)
    with zipfile.ZipFile(io.BytesIO(raw_zip)) as zf:
        csv_names = [n for n in zf.namelist() if n.lower().endswith(".csv")]
        if not csv_names:
            raise ValueError(f"No CSV found in OGSRL ZIP. Contents: {zf.namelist()}")
        log.info("CSV entries in ZIP: %s", csv_names)
        with zf.open(csv_names[0]) as raw:
            text = raw.read().decode("cp1252", errors="replace")

    reader = csv.DictReader(io.StringIO(text))
    return _parse_ogsrl_rows(reader)


def load_from_csv(csv_path: Path) -> list[OGSRLWell]:
    """Load from a pre-downloaded OGSRL CSV file (Windows-1252 encoding)."""
    log.info("Loading OGSRL wells from local CSV: %s", csv_path)
    with open(csv_path, "r", encoding="cp1252", errors="replace") as f:
        reader = csv.DictReader(f)
        return _parse_ogsrl_rows(reader)


def load_or_fetch_wells(
    cache_path: Path = DEFAULT_CACHE,
    force_fetch: bool = False,
) -> list[OGSRLWell]:
    """Return wells from local cache, or download+cache from OGSRL."""
    cache_path = Path(cache_path)

    if cache_path.exists() and not force_fetch:
        log.info("Loading cached wells from %s", cache_path)
        with open(cache_path, "r", encoding="utf-8") as f:
            data = json.load(f)
        wells = [OGSRLWell(**w) for w in data]
        log.info("Loaded %d cached wells", len(wells))
        return wells

    wells = fetch_ogsrl_from_web()
    cache_path.parent.mkdir(parents=True, exist_ok=True)
    with open(cache_path, "w", encoding="utf-8") as f:
        json.dump([asdict(w) for w in wells], f, indent=2)
    log.info("Cached %d wells → %s", len(wells), cache_path)
    return wells


# ─────────────────────────────────────────────────────────────────────────────
# Candidate loader — handles multiple input JSON schemas
# ─────────────────────────────────────────────────────────────────────────────

def load_candidates(json_path: Path) -> list[dict]:
    """Load mag candidates from JSON.  Handles:
      - Flat list: [{"center_lat": ..., "center_lon": ...}, ...]
      - MB2 format: {"wreck": ..., "detections": [...]}
      - Scan output: {"candidates": [...]} / {"results": [...]}
    """
    with open(json_path, "r", encoding="utf-8") as f:
        data = json.load(f)

    if isinstance(data, list):
        return data

    for key in ("detections", "candidates", "results", "anomalies", "features"):
        if key in data:
            cands = data[key]
            src   = data.get("wreck", data.get("name", key))
            log.info("Loaded %d candidates (key=%r, source=%r)", len(cands), key, src)
            return cands

    # Single candidate or top-level flat dict
    if "center_lat" in data or "lat" in data or "latitude" in data:
        return [data]

    log.warning("Unrecognized JSON structure in %s — trying top-level values", json_path)
    return list(data.values()) if isinstance(data, dict) else []


def _norm(c: dict) -> dict:
    """Normalize field aliases across different scan output schemas."""
    out = dict(c)
    # lat / lon
    for alias, canon in (
        ("lat",       "center_lat"),
        ("latitude",  "center_lat"),
        ("lon",       "center_lon"),
        ("longitude", "center_lon"),
    ):
        if alias in c and canon not in c:
            out[canon] = c[alias]
    # score
    for alias, canon in (
        ("z_score",  "composite_score"),
        ("zscore",   "composite_score"),
        ("score",    "composite_score"),
    ):
        if alias in c and canon not in c:
            out[canon] = c[alias]
    return out


# ─────────────────────────────────────────────────────────────────────────────
# Nearest-well lookup
# ─────────────────────────────────────────────────────────────────────────────

def find_nearest_well(
    lat: float,
    lon: float,
    wells: list[OGSRLWell],
    search_radius_m: float = 20_000.0,
) -> tuple[Optional[OGSRLWell], float]:
    """Return (nearest_well, distance_m).  Returns (None, inf) if nothing within radius."""
    best_well: Optional[OGSRLWell] = None
    best_dist = float("inf")

    for w in wells:
        # Fast reject: ~1° lat ≈ 111 km, so skip if obviously too far
        dlat = abs(lat - w.lat)
        if dlat * 111_000 > search_radius_m * 1.5:
            continue
        d = haversine_m(lat, lon, w.lat, w.lon)
        if d < best_dist:
            best_dist = d
            best_well = w

    if best_dist > search_radius_m:
        return None, best_dist
    return best_well, best_dist


def wells_within_radius(
    lat: float,
    lon: float,
    wells: list[OGSRLWell],
    radius_m: float,
) -> list[tuple[OGSRLWell, float]]:
    """Return all wells within *radius_m*, sorted by distance asc."""
    hits: list[tuple[OGSRLWell, float]] = []
    for w in wells:
        dlat = abs(lat - w.lat)
        if dlat * 111_000 > radius_m * 1.5:
            continue
        d = haversine_m(lat, lon, w.lat, w.lon)
        if d <= radius_m:
            hits.append((w, d))
    hits.sort(key=lambda x: x[1])
    return hits


# ─────────────────────────────────────────────────────────────────────────────
# Side-pull detection
# ─────────────────────────────────────────────────────────────────────────────

def compute_side_pull(
    cand_lat: float,
    cand_lon: float,
    long_axis_bearing: float,
    nearest_well: Optional[OGSRLWell],
) -> tuple[float, float, bool]:
    """Detect side pull: does the anomaly's long axis point toward the nearest well?

    At aeromagnetic resolution the anomaly centroid is dragged along the flight
    line toward the dominant point source (the well casing).  The elongation axis
    therefore points toward the well rather than aligning with a vessel heading.

    Returns (bearing_to_well, angular_diff_deg, side_pull_flag).
      angular_diff_deg is the axis-wise angular difference (0–90°); lower = more aligned.
    """
    if nearest_well is None:
        return 0.0, 90.0, False

    brg  = bearing_deg(cand_lat, cand_lon, nearest_well.lat, nearest_well.lon)
    diff = angular_diff_axis(long_axis_bearing, brg)
    flag = diff < SIDE_PULL_CONE_DEG
    return brg, diff, flag


# ─────────────────────────────────────────────────────────────────────────────
# Orientation analysis
# ─────────────────────────────────────────────────────────────────────────────

def classify_orientation(
    long_axis_deg: float,
    aspect_ratio: float,
) -> tuple[str, float, float, float]:
    """Classify the anomaly's long-axis orientation.

    Compares against:
      • geomagnetic strike   (~170° / N-S for Erie 2015)
      • Ontario lot grid     (0° / 90° cardinal axes)
      • Erie shipping lane   (~075°)

    Returns (orientation_flag, geomag_diff, grid_diff, lane_diff).
    """
    geomag_diff = angular_diff_axis(long_axis_deg, GEOMAG_STRIKE_DEG)
    grid_diff   = min(angular_diff_axis(long_axis_deg, g) for g in ONTARIO_GRID_BEARINGS)
    lane_diff   = angular_diff_axis(long_axis_deg, SHIPPING_LANE_BEARING)

    flag = "unknown"
    if aspect_ratio >= PIPELINE_ASPECT_RATIO:
        if grid_diff < GRID_TOL_DEG:
            flag = "grid_pipeline"       # strongly elongated + aligned to well grid
        elif lane_diff < LANE_TOL_DEG:
            flag = "shipping_lane"       # elongated but aligned to vessel route
        elif geomag_diff < GEOMAG_TOL_DEG:
            flag = "geomagnetic"         # elongated along field — possibly geological
        else:
            flag = "elongated_unknown"
    else:
        # Not highly elongated — orientation is a weaker signal
        if geomag_diff < GEOMAG_TOL_DEG:
            flag = "geomagnetic"
        elif grid_diff < GRID_TOL_DEG:
            flag = "grid"
        elif lane_diff < LANE_TOL_DEG:
            flag = "lane"

    return flag, geomag_diff, grid_diff, lane_diff


# ─────────────────────────────────────────────────────────────────────────────
# Scoring
# ─────────────────────────────────────────────────────────────────────────────

def compute_well_score(res: DiscriminatorResult) -> tuple[float, list[str]]:
    """Compute a 0.0–1.0 score for how likely the anomaly is a well artifact.

    Score weights:
      +0.40  within PROX_RADIUS_M of a well  (dominant signal)
      +0.25  side-pull flag (long axis pointing at a well)
      +0.20  pipeline elongation (aspect_ratio > PIPELINE_ASPECT_RATIO)
      +0.15  grid-axis alignment when elongated (Ontario lot grid pattern)
      −0.15  shipping-lane alignment (suggests an actual vessel)
    """
    score   = 0.0
    reasons: list[str] = []

    if res.within_prox:
        score += 0.40
        reasons.append(
            f"within {res.nearest_well_m:.0f} m of {res.nearest_well_class} well "
            f"'{res.nearest_well_name}' [{res.nearest_well_id}]"
        )

    if res.side_pull_flag:
        score += 0.25
        reasons.append(
            f"side-pull: long axis {res.anomaly_long_axis_bearing:.0f}° aligns with "
            f"bearing to well {res.bearing_to_nearest_well:.0f}° "
            f"(diff={res.side_pull_angular_diff:.1f}°)"
        )

    if res.is_pipeline_elongation:
        score += 0.20
        reasons.append(
            f"pipeline elongation: aspect ratio {res.aspect_ratio:.2f} "
            f"(threshold {PIPELINE_ASPECT_RATIO})"
        )
        if res.grid_alignment_deg < GRID_TOL_DEG:
            score += 0.15
            reasons.append(
                f"grid-aligned: {res.grid_alignment_deg:.1f}° from Ontario cardinal grid"
            )
    elif res.grid_alignment_deg < GRID_TOL_DEG:
        score += 0.08
        reasons.append(
            f"grid orientation: {res.grid_alignment_deg:.1f}° from Ontario lot grid"
        )

    if res.lane_alignment_deg < LANE_TOL_DEG:
        score -= 0.15
        reasons.append(
            f"shipping-lane alignment: {res.lane_alignment_deg:.1f}° from Erie ENE lane "
            f"(075°) — consistent with vessel"
        )

    return max(0.0, min(1.0, score)), reasons


def classify_result(well_score: float) -> str:
    """Map well_score to a classification label."""
    if well_score >= 0.75:
        return "well_pull"
    elif well_score >= 0.55:
        return "pipeline"
    elif well_score >= 0.35:
        return "suspicious"
    elif well_score <= 0.10:
        return "wreck"
    else:
        return "unknown"


# ─────────────────────────────────────────────────────────────────────────────
# Main discrimination loop
# ─────────────────────────────────────────────────────────────────────────────

def discriminate(
    candidates: list[dict],
    wells: list[OGSRLWell],
) -> list[DiscriminatorResult]:
    """Run the full discrimination pipeline on every candidate."""
    results: list[DiscriminatorResult] = []
    no_coords = 0

    for raw in candidates:
        c = _norm(raw)
        try:
            clat = float(c.get("center_lat", 0) or 0)
            clon = float(c.get("center_lon", 0) or 0)
        except (ValueError, TypeError):
            no_coords += 1
            continue
        if clat == 0.0 or clon == 0.0:
            no_coords += 1
            continue

        width_m  = float(c.get("width_m",  0) or 0)
        height_m = float(c.get("height_m", 0) or 0)

        res = DiscriminatorResult(
            label_id          = int(c.get("label_id", c.get("id", 0)) or 0),
            center_lat        = clat,
            center_lon        = clon,
            composite_score   = float(c.get("composite_score", 0) or 0),
            dipole_score      = float(c.get("dipole_score",    c.get("_dipole_score", 0)) or 0),
            tier              = str(c.get("tier",  c.get("_tier",  "")) or ""),
            width_m           = width_m,
            height_m          = height_m,
            amplitude_peak_nT = float(c.get("amplitude_peak_abs", c.get("amplitude", 0)) or 0),
            z_score           = float(c.get("z_score", c.get("zscore", 0)) or 0),
        )

        # ── 1. Nearest well ──────────────────────────────────────────────────
        nearest, dist = find_nearest_well(clat, clon, wells)
        if nearest is not None:
            res.nearest_well_id    = nearest.well_id
            res.nearest_well_name  = nearest.name
            res.nearest_well_class = nearest.well_class
            res.nearest_well_lat   = nearest.lat
            res.nearest_well_lon   = nearest.lon
            res.nearest_well_m     = dist
            res.within_prox        = dist <= PROX_RADIUS_M
        else:
            res.nearest_well_m = dist  # may be >search_radius or inf

        # ── 2. Anomaly long axis ─────────────────────────────────────────────
        res.anomaly_long_axis_bearing = long_axis_bearing_from_bbox(width_m, height_m)
        if width_m > 0 and height_m > 0:
            res.aspect_ratio = max(width_m, height_m) / min(width_m, height_m)
        res.is_pipeline_elongation = res.aspect_ratio >= PIPELINE_ASPECT_RATIO

        # ── 3. Side-pull ─────────────────────────────────────────────────────
        (
            res.bearing_to_nearest_well,
            res.side_pull_angular_diff,
            res.side_pull_flag,
        ) = compute_side_pull(clat, clon, res.anomaly_long_axis_bearing, nearest)

        # ── 4. Orientation ───────────────────────────────────────────────────
        (
            res.orientation_flag,
            res.geomag_alignment_deg,
            res.grid_alignment_deg,
            res.lane_alignment_deg,
        ) = classify_orientation(res.anomaly_long_axis_bearing, res.aspect_ratio)

        # ── 5. Score + classify ──────────────────────────────────────────────
        res.well_score, res.reasons = compute_well_score(res)
        res.classification = classify_result(res.well_score)

        results.append(res)

    if no_coords:
        log.warning("Skipped %d candidates with missing / zero coordinates", no_coords)

    return results


# ─────────────────────────────────────────────────────────────────────────────
# Output writers
# ─────────────────────────────────────────────────────────────────────────────

def write_results_json(results: list[DiscriminatorResult], out_path: Path) -> None:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump([asdict(r) for r in results], f, indent=2)
    log.info("Wrote %d annotated results → %s", len(results), out_path)


def write_summary(results: list[DiscriminatorResult], out_path: Path) -> None:
    counts     = Counter(r.classification for r in results)
    prox_n     = sum(1 for r in results if r.within_prox)
    pull_n     = sum(1 for r in results if r.side_pull_flag)
    elong_n    = sum(1 for r in results if r.is_pipeline_elongation)
    wreck_cands = sorted(results, key=lambda r: r.well_score)[:15]
    well_cands  = sorted(
        [r for r in results if r.well_score >= 0.5], key=lambda r: -r.well_score
    )[:15]

    W = 78
    lines: list[str] = [
        "═" * W,
        "  OGSRL Well / Pipeline Discrimination Summary",
        f"  Geomagnetic ref: declination {IGRF_2015_ERIE['declination_deg']}°  "
        f"inclination {IGRF_2015_ERIE['inclination_deg']}°  epoch {IGRF_2015_ERIE['epoch']}",
        "═" * W,
        f"  Total candidates analyzed    : {len(results):>6}",
        f"  Within {PROX_RADIUS_M:.0f} m of a well       : {prox_n:>6}",
        f"  Side-pull flagged            : {pull_n:>6}",
        f"  Pipeline elongation          : {elong_n:>6}",
        "",
        "  Classification breakdown:",
    ]
    for cls, n in sorted(counts.items(), key=lambda x: -x[1]):
        bar = "█" * min(40, int(40 * n / max(1, len(results))))
        lines.append(f"    {cls:<20s}  {n:>6}  {bar}")

    lines += [
        "",
        "  Top 15 wreck candidates  (lowest well_score — most likely NOT a well)",
        "  " + "-" * (W - 2),
        f"  {'id':>8}  {'lat':>10}  {'lon':>11}  {'well_sc':>8}  "
        f"{'near_m':>8}  {'z_score':>8}  classification",
    ]
    for r in wreck_cands:
        lines.append(
            f"  {r.label_id:>8}  {r.center_lat:>10.5f}  {r.center_lon:>11.5f}  "
            f"{r.well_score:>8.3f}  {r.nearest_well_m:>8.0f}  "
            f"{r.z_score:>8.2f}  {r.classification}"
        )

    lines += [
        "",
        "  Top 15 well/pipeline candidates  (highest well_score)",
        "  " + "-" * (W - 2),
        f"  {'id':>8}  {'lat':>10}  {'lon':>11}  {'well_sc':>8}  "
        f"{'near_m':>8}  {'well_id':>12}  nearest well",
    ]
    for r in well_cands:
        lines.append(
            f"  {r.label_id:>8}  {r.center_lat:>10.5f}  {r.center_lon:>11.5f}  "
            f"{r.well_score:>8.3f}  {r.nearest_well_m:>8.0f}  "
            f"{r.nearest_well_id:>12}  {r.nearest_well_name}"
        )

    lines.append("═" * W)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    for line in lines:
        log.info(line)


# ─────────────────────────────────────────────────────────────────────────────
# CLI
# ─────────────────────────────────────────────────────────────────────────────

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="ogsrl_well_discriminator.py",
        description="Flag mag anomaly candidates as well/pipeline artifacts using OGSRL data.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    p.add_argument(
        "--candidates", "-c", required=True, metavar="JSON",
        help="Candidates JSON (MB2 format, HC scan output, or flat list).",
    )
    p.add_argument(
        "--fetch", action="store_true",
        help="Force re-download of OGSRL CSV from the web (ignores cache).",
    )
    p.add_argument(
        "--cache", default=str(DEFAULT_CACHE), metavar="JSON",
        help=f"OGSRL wells cache path (default: {DEFAULT_CACHE}).",
    )
    p.add_argument(
        "--csv", default=None, metavar="CSV",
        help="Pre-downloaded OGSRL CSV file (skip web fetch entirely).",
    )
    p.add_argument(
        "--output", "-o", default=None, metavar="JSON",
        help="Output JSON path (default: <candidates_stem>_ogsrl_discriminated.json).",
    )
    p.add_argument(
        "--prox-radius", type=float, default=PROX_RADIUS_M, metavar="M",
        help=f"Well proximity radius in metres (default: {PROX_RADIUS_M:.0f}).",
    )
    p.add_argument(
        "--side-pull-cone", type=float, default=SIDE_PULL_CONE_DEG, metavar="DEG",
        help=f"Side-pull detection cone half-angle in degrees (default: {SIDE_PULL_CONE_DEG}).",
    )
    p.add_argument(
        "--debug", action="store_true",
        help="Enable DEBUG-level logging.",
    )
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    if args.debug:
        logging.getLogger().setLevel(logging.DEBUG)

    # Apply any CLI overrides to module-level thresholds
    global PROX_RADIUS_M, SIDE_PULL_CONE_DEG
    PROX_RADIUS_M      = args.prox_radius
    SIDE_PULL_CONE_DEG = args.side_pull_cone

    # ── Load wells ──────────────────────────────────────────────────────────
    if args.csv:
        wells = load_from_csv(Path(args.csv))
    else:
        wells = load_or_fetch_wells(
            cache_path=Path(args.cache),
            force_fetch=args.fetch,
        )

    if not wells:
        log.error(
            "No wells loaded.  "
            "Use --fetch to download from OGSRL or --csv <path> for a local file."
        )
        return 1

    gas   = sum(1 for w in wells if w.well_class == "GAS")
    oil   = sum(1 for w in wells if w.well_class == "OIL")
    salt  = sum(1 for w in wells if w.well_class == "SALT")
    lake  = sum(1 for w in wells if w.is_lake_erie)
    log.info(
        "Well inventory: %d total  (GAS=%d  OIL=%d  SALT=%d  other=%d  offshore=%d)",
        len(wells), gas, oil, salt, len(wells) - gas - oil - salt, lake,
    )

    # ── Load candidates ─────────────────────────────────────────────────────
    cand_path  = Path(args.candidates)
    candidates = load_candidates(cand_path)
    if not candidates:
        log.error("No candidates found in %s", cand_path)
        return 1
    log.info("Loaded %d candidates from %s", len(candidates), cand_path)

    # ── Discriminate ────────────────────────────────────────────────────────
    results = discriminate(candidates, wells)
    log.info("Discrimination complete: %d results", len(results))

    # ── Write output ────────────────────────────────────────────────────────
    stem     = cand_path.stem
    out_json = (
        Path(args.output) if args.output
        else cand_path.parent / f"{stem}_ogsrl_discriminated.json"
    )
    out_txt = out_json.with_suffix(".txt")

    write_results_json(results, out_json)
    write_summary(results, out_txt)

    return 0


if __name__ == "__main__":
    sys.exit(main())
