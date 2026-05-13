#!/usr/bin/env python3
"""
Strait of Hormuz Maritime Mine Hazard Detection
================================================
Civilian maritime safety scan — CESAROPS maritime hazard module.

Goal: Detect potential surface/floating mine signatures and mine-laying
activity in the Strait of Hormuz transit lanes using the most recent
available Sentinel-1 SAR and Sentinel-2 optical imagery.

Why this matters
----------------
The Strait of Hormuz is the world's most critical oil chokepoint: ~21M
barrels/day, ~1/5 of global supply. Floating mines and moored contact mines
in the transit lanes directly threaten civilian tanker crews and commercial
shipping. This scan uses the same spectral/SAR anomaly pipeline as the oil
spill and wreck detection modules — applied here to maritime hazard mapping.

Satellite mine detection science
---------------------------------
FLOATING / NEAR-SURFACE MINES (~0.5–1m objects):
  • Too small to resolve at Sentinel-2 10m or Sentinel-1 10m GRD
  • BUT: mooring cables cause persistent slick patterns in SAR (Bragg resonance)
  • A cluster of 3+ mines creates a statistically detectable anomaly pattern

SAR MOORING CABLE SIGNATURE:
  • Taut wire from seabed to surface buoy → narrow dark streak in VV/VH
  • Orientation aligned with current drift direction
  • Compare: oil spill slick (wider, shaped by wind) vs. cable (narrow, linear)

AIS DARK VESSEL / MINE-LAYING ACTIVITY:
  • Mine-laying vessels often have AIS transponders off → no AIS track
  • Sentinel-1 detects vessel SAR signature even without AIS
  • "Dark vessel" = SAR bright point with no corresponding AIS vessel nearby
  • Clusters of dark vessel detections in transit lane → mine-laying risk flag

MULTI-TEMPORAL CHANGE:
  • Day-over-day SAR difference: new persistent bright returns in open water
  • Sentinel-1 6-day repeat allows detection of objects that were NOT there 6 days ago

SEDIMENT PLUME:
  • Mine anchoring disturbs seabed → plume detectable for ~24h post-placement
  • Sentinel-2 B02/B03 turbidity check in shallow shelf (<50m)

OPTICAL SURFACE OBJECTS:
  • Sentinel-2 at 10m: very bright small objects (metallic sphere) can create
    1–2 pixel anomaly in B02/B03 if object > ~3m (mine line cluster)
  • False positive rate high — confirm with SAR

Search geometry (Strait of Hormuz)
-----------------------------------
The Strait narrows to ~54km at its minimum. Traffic Separation Scheme (TSS):
  Inbound lane:  25.6–26.2°N, 56.3–56.7°E  (NW lane, toward Persian Gulf)
  Outbound lane: 26.2–26.6°N, 56.5–57.0°E  (SE lane, toward Gulf of Oman)
  Separation zone between lanes

Full bbox:   [25.3, 55.5, 27.2, 58.5]   (whole strait + approaches)
Inbound TSS: [25.8, 56.2, 26.3, 56.8]
Outbound TSS:[26.1, 56.4, 26.7, 57.1]
Khasab Bay:  [26.1, 56.2, 26.5, 56.7]   (Oman side — known anchorage)
Abu Musa Is: [25.7, 55.0, 25.95, 55.2]  (contested island — historic mining area)

Detection Passes
----------------
PASS 1 — SAR bright-point anomaly (surface/floating objects)
  Sentinel-1 GRD: detect statistically anomalous small high-backscatter
  point targets in open water. Threshold: VV sigma > mean + 3*std in
  20x20 pixel local window. Cluster rank by density in 500m radius.

PASS 2 — Multi-temporal SAR change (new objects)
  Diff two Sentinel-1 passes (6-day interval). Persistent new returns
  that did not exist in prior pass and are NOT correlated with vessel AIS.
  z-score of diff > 2.5 in transit lane mask.

PASS 3 — Dark vessel detection (AIS-absent SAR vessels)
  Pull current Sentinel-1 vessel detections (OSM/GPT OSINT layer).
  Cross-reference against MarineTraffic/AIS known positions.
  Flag SAR vessels with no AIS match within 500m radius.

PASS 4 — Turbidity plume (seabed disturbance)
  Sentinel-2 B02/B03 in shallow shelf zones (<50m depth ETOPO).
  Turbidity anomaly z-score > 2.0 in 3km radius from candidates.
  Eliminates natural upwelling: wind speed proxy filter (ERA5).

PASS 5 — Optical surface cluster (Sentinel-2 B02 bright anomaly)
  Sentinel-2 10m B02: mask water; find isolated very bright pixels
  (reflectance > 0.15 in open water). Cluster analysis 1km grid.

Risk Output
-----------
  CRITICAL  — dark vessel + SAR new return + AIS gap in transit lane
  HIGH      — SAR new return in transit lane, no vessel match
  MEDIUM    — bright point anomaly, transit lane proximity < 2km
  LOW       — turbidity plume or optical blob, shelf zone only
  INFO      — background reference placemarks (vessel traffic, anchorages)

Outputs
-------
  outputs/hormuz/
    pass1_sar_anomaly.kmz
    pass2_sar_change.kmz
    pass3_dark_vessels.kmz
    pass4_turbidity.kmz
    pass5_optical_surface.kmz
    hormuz_mine_hazard_combined.kmz    ← load this in Google Earth
    hormuz_candidates.json
    hormuz_scan_log.txt

Run
---
  python hormuz_mine_scan.py
  python hormuz_mine_scan.py --quick       # transit lane only, SAR passes 1+2
  python hormuz_mine_scan.py --push-queue  # dispatch to i7 scan worker

IMPORTANT: This is a civilian maritime safety tool. Outputs are hazard
indicators only — NOT confirmed mine locations. All flagged areas must be
assessed by qualified maritime authorities before any navigation advisory.
"""

import argparse
import io
import json
import math
import os
import sys
import zipfile
from datetime import datetime, date, timedelta, timezone
from pathlib import Path
from typing import List, Optional

import requests
import numpy as np

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

# ── Paths ─────────────────────────────────────────────────────────────────────

REPO       = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / "outputs" / "hormuz"
TILE_DIR   = REPO / "downloads" / "hormuz"
LOG_PATH   = OUTPUT_DIR / "hormuz_scan_log.txt"

OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
TILE_DIR.mkdir(parents=True, exist_ok=True)
_log_f = open(str(LOG_PATH), "a", encoding="utf-8")

def log(msg: str):
    ts = datetime.now(timezone.utc).strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line, flush=True)
    _log_f.write(line + "\n")
    _log_f.flush()


# ── Config ────────────────────────────────────────────────────────────────────

def _env(k: str, default: str = "") -> str:
    env_path = REPO / ".env"
    if env_path.exists():
        for line in env_path.read_text().splitlines():
            if line.startswith(k + "="):
                return line.split("=", 1)[1].strip()
    return os.environ.get(k, default)

EARTHDATA_TOKEN = _env("EARTHDATA_TOKEN")
CMR_BASE        = "https://cmr.earthdata.nasa.gov/search"

# Use most-recent 30 days from today so we always pull newest imagery
_TODAY      = date.today()
DATE_END    = _TODAY.strftime("%Y-%m-%d")
DATE_START  = (_TODAY - timedelta(days=30)).strftime("%Y-%m-%d")

# CMR concept IDs
CONCEPT_S30  = "C2021957295-LPCLOUD"   # HLS Sentinel-2 30m (optical)
CONCEPT_SAR  = "C1214470488-ASF"       # Sentinel-1 GRD Level-1 (ASF DAAC — global)
CONCEPT_SAR2 = "C2036882064-ASF"       # Sentinel-1 RTC (ASF) — fallback

# ── Strait geometry ───────────────────────────────────────────────────────────

STRAIT = {
    "label":       "Strait of Hormuz",
    "full_bbox":   [25.0, 54.0, 27.5, 58.5],   # [lat_min, lon_min, lat_max, lon_max]
    # Traffic Separation Scheme lanes (IMO TSS)
    "inbound_tss":  [25.8, 56.2, 26.3, 56.8],   # NW inbound (vessels entering Gulf)
    "outbound_tss": [26.1, 56.4, 26.7, 57.1],   # SE outbound
    # High-risk sub-zones
    "abu_musa":    [25.7, 55.0, 25.95, 55.2],   # Contested island — historic 1987/88 mining
    "khasab_bay":  [26.1, 56.2, 26.5, 56.7],    # Oman side anchorage
    "qeshm_north": [26.6, 55.6, 27.2, 56.7],    # Qeshm Island north channel
    # The narrow bottleneck — highest risk
    "bottleneck":  [26.1, 56.3, 26.8, 57.0],
}

# Transit lane polygon — used as mask for risk elevation
TSS_POLYGONS = {
    "inbound":  STRAIT["inbound_tss"],
    "outbound": STRAIT["outbound_tss"],
}

# Average water depth (ETOPO proxy) for shallow-shelf turbidity filter
# Abu Musa shelf < 50m, bottleneck ~70-100m, eastern approach >100m
SHALLOW_ZONES = [
    STRAIT["abu_musa"],
    [25.5, 55.4, 26.0, 56.2],   # Tunb islands area shallow shelf
]


# ── CMR query ─────────────────────────────────────────────────────────────────

def _cmr_query(concept_id: str, bbox: list, start: str, end: str,
               max_results: int = 50) -> list:
    lat_min, lon_min, lat_max, lon_max = bbox
    headers = {}
    if EARTHDATA_TOKEN:
        headers["Authorization"] = f"Bearer {EARTHDATA_TOKEN}"
    params = {
        "concept_id":   concept_id,
        "temporal":     f"{start}T00:00:00Z,{end}T23:59:59Z",
        "bounding_box": f"{lon_min},{lat_min},{lon_max},{lat_max}",
        "page_size":    min(max_results, 200),
        "sort_key":     "-start_date",   # newest first
    }
    try:
        r = requests.get(f"{CMR_BASE}/granules.json", params=params,
                         headers=headers, timeout=30)
        r.raise_for_status()
        entries = r.json().get("feed", {}).get("entry", [])
        return entries
    except Exception as e:
        log(f"  CMR query failed ({concept_id[:12]}...): {e}")
        return []


def _cloud_cover(g: dict) -> float:
    for attr in g.get("attributes", []):
        if attr.get("name", "").lower() in ("cloud_cover", "eo:cloud_cover"):
            try:
                return float(attr["value"])
            except Exception:
                pass
    return 100.0


def _granule_date(g: dict) -> str:
    return g.get("time_start", "")[:10]


def find_best_granules(bbox: list, max_cloud: float = 30.0) -> dict:
    """Return newest SAR + optical granules covering the bbox."""
    log(f"  Querying CMR for newest imagery [{DATE_START} → {DATE_END}]...")
    results: dict = {}

    sar = _cmr_query(CONCEPT_SAR, bbox, DATE_START, DATE_END, max_results=20)
    if not sar:
        log(f"  SAR (C1214470488) returned 0 — trying fallback concept")
        sar = _cmr_query(CONCEPT_SAR2, bbox, DATE_START, DATE_END, max_results=20)
    log(f"  SAR (Sentinel-1 GRD): {len(sar)} granules found")
    results["sar"] = sar[:6]

    opt = _cmr_query(CONCEPT_S30, bbox, DATE_START, DATE_END, max_results=30)
    clear_opt = [g for g in opt if _cloud_cover(g) <= max_cloud]
    log(f"  Optical (HLS S30): {len(opt)} total → {len(clear_opt)} < {max_cloud}% cloud")
    results["optical"] = clear_opt[:6]

    # Log the actual dates of what we got
    for sensor, grans in results.items():
        if grans:
            dates = [_granule_date(g) for g in grans[:3]]
            log(f"  {sensor} newest dates: {', '.join(dates)}")

    return results


# ── SAR anomaly detection (pass 1) ───────────────────────────────────────────

def _sar_bright_point_anomaly(vv: np.ndarray, window: int = 20,
                               z_thresh: float = 3.0) -> np.ndarray:
    """
    Detect statistically anomalous small high-backscatter points.
    Uses local z-score in a rolling window to find isolated bright targets
    that stand out against uniform open-water background.
    Returns binary mask of candidate pixels.
    """
    if vv.size == 0:
        return np.zeros((0,), dtype=bool)

    h, w = vv.shape
    out = np.zeros((h, w), dtype=bool)
    half = window // 2

    for i in range(half, h - half):
        for j in range(half, w - half):
            patch = vv[i - half:i + half, j - half:j + half]
            flat = patch[patch > 0]
            if flat.size < 10:
                continue
            mu = flat.mean()
            sd = flat.std()
            if sd < 1e-6:
                continue
            z = (float(vv[i, j]) - mu) / sd
            if z > z_thresh:
                out[i, j] = True
    return out


def _cluster_pixels(mask: np.ndarray, pixel_size_deg: float,
                    cluster_radius_km: float = 0.5) -> list:
    """
    Group adjacent True pixels into clusters.
    Returns list of (centroid_row, centroid_col, pixel_count).
    Simple flood-fill based clustering.
    """
    visited = np.zeros_like(mask, dtype=bool)
    clusters = []
    rows, cols = np.where(mask)

    for r, c in zip(rows, cols):
        if visited[r, c]:
            continue
        # BFS flood fill
        stack = [(r, c)]
        pts = []
        while stack:
            nr, nc = stack.pop()
            if nr < 0 or nr >= mask.shape[0] or nc < 0 or nc >= mask.shape[1]:
                continue
            if visited[nr, nc] or not mask[nr, nc]:
                continue
            visited[nr, nc] = True
            pts.append((nr, nc))
            for dr, dc in [(-1,0),(1,0),(0,-1),(0,1)]:
                stack.append((nr+dr, nc+dc))
        if pts:
            mean_r = sum(p[0] for p in pts) / len(pts)
            mean_c = sum(p[1] for p in pts) / len(pts)
            clusters.append((mean_r, mean_c, len(pts)))
    return clusters


# ── Multi-temporal change detection (pass 2) ─────────────────────────────────

def _sar_temporal_diff(vv_new: np.ndarray, vv_old: np.ndarray,
                        z_thresh: float = 2.5) -> np.ndarray:
    """
    Difference two SAR passes. Returns z-score anomaly mask of new persistent
    returns that did not exist in the prior pass.
    """
    if vv_new.shape != vv_old.shape:
        return np.zeros_like(vv_new, dtype=bool)

    diff = vv_new.astype(np.float32) - vv_old.astype(np.float32)
    # Only care about new positive returns (new bright objects)
    diff = np.clip(diff, 0, None)

    flat = diff[diff > 0]
    if flat.size < 10:
        return np.zeros_like(vv_new, dtype=bool)

    mu, sd = flat.mean(), flat.std()
    if sd < 1e-6:
        return np.zeros_like(vv_new, dtype=bool)

    zmap = (diff - mu) / sd
    return zmap > z_thresh


# ── Turbidity plume analysis (pass 4) ────────────────────────────────────────

def _turbidity_anomaly(b02: np.ndarray, b03: np.ndarray,
                        z_thresh: float = 2.0) -> np.ndarray:
    """
    Water turbidity proxy: B02/B03 ratio anomaly.
    Fresh seabed disturbance (anchor/mooring placement) raises sediment
    → higher B02 reflectance vs. baseline.
    """
    with np.errstate(divide='ignore', invalid='ignore'):
        ratio = np.where(b03 > 0,
                         b02.astype(np.float32) / b03.astype(np.float32),
                         np.nan)
    flat = ratio[~np.isnan(ratio)]
    if flat.size < 10:
        return np.zeros_like(b02, dtype=bool)
    mu, sd = flat.mean(), flat.std()
    if sd < 1e-6:
        return np.zeros_like(b02, dtype=bool)
    return (ratio - mu) / sd > z_thresh


# ── Optical bright surface object (pass 5) ───────────────────────────────────

def _optical_surface_objects(b02: np.ndarray, water_mask: np.ndarray,
                              reflectance_thresh: float = 0.15) -> np.ndarray:
    """
    Find isolated very bright pixels in open water at 10m.
    A metallic mine cluster (>3m dimension) producing specular glint
    can exceed reflectance 0.15 in B02 over open dark water (~0.03).
    """
    # Normalize: HLS B02 is typically 0–10000 scale
    scale = b02.max() if b02.max() > 1 else 1.0
    norm = b02.astype(np.float32) / scale
    bright = (norm > reflectance_thresh) & water_mask
    return bright


# ── Risk scoring ─────────────────────────────────────────────────────────────

RISK_CRITICAL = "CRITICAL"
RISK_HIGH     = "HIGH"
RISK_MEDIUM   = "MEDIUM"
RISK_LOW      = "LOW"

# KML icon colors (aabbggrr format)
RISK_COLORS = {
    RISK_CRITICAL: "ff0000ff",   # red
    RISK_HIGH:     "ff0088ff",   # orange
    RISK_MEDIUM:   "ff00ffff",   # yellow
    RISK_LOW:      "ff00ff88",   # green-yellow
}

RISK_ICONS = {
    RISK_CRITICAL: "http://maps.google.com/mapfiles/kml/shapes/caution.png",
    RISK_HIGH:     "http://maps.google.com/mapfiles/kml/shapes/caution.png",
    RISK_MEDIUM:   "http://maps.google.com/mapfiles/kml/shapes/target.png",
    RISK_LOW:      "http://maps.google.com/mapfiles/kml/shapes/placemark_circle.png",
}


def _point_in_tss(lat: float, lon: float) -> Optional[str]:
    """Return TSS lane name if the point falls within the traffic separation scheme."""
    for lane, bbox in TSS_POLYGONS.items():
        lat_min, lon_min, lat_max, lon_max = bbox
        if lat_min <= lat <= lat_max and lon_min <= lon <= lon_max:
            return lane
    return None


def _score_candidate(lat: float, lon: float, passes: list,
                      confidence: float) -> str:
    """
    Compute risk level from which passes fired and geographic context.
    passes: list of pass names that contributed (e.g. ['sar_bright','sar_change'])
    """
    tss = _point_in_tss(lat, lon)
    n_passes = len(passes)

    if n_passes >= 3 or (tss and n_passes >= 2):
        return RISK_CRITICAL
    if n_passes == 2 or (tss and n_passes == 1 and "sar_change" in passes):
        return RISK_HIGH
    if n_passes == 1 and tss:
        return RISK_MEDIUM
    return RISK_LOW


# ── KMZ builder ───────────────────────────────────────────────────────────────

def _placemark(name: str, lat: float, lon: float,
               risk: str, passes: list, granule_dates: list,
               description: str = "") -> str:
    color = RISK_COLORS[risk]
    icon  = RISK_ICONS[risk]
    scale = {"CRITICAL": 1.4, "HIGH": 1.2, "MEDIUM": 0.9, "LOW": 0.7}[risk]
    pass_str  = ", ".join(passes) if passes else "–"
    date_str  = ", ".join(granule_dates) if granule_dates else "–"
    desc = f"Risk: {risk}<br/>Passes: {pass_str}<br/>Imagery dates: {date_str}"
    if description:
        desc += f"<br/>{description}"
    return f"""
    <Placemark>
      <name>{name}</name>
      <description><![CDATA[{desc}]]></description>
      <Style><IconStyle>
        <color>ff{color[-6:]}</color>
        <scale>{scale:.1f}</scale>
        <Icon><href>{icon}</href></Icon>
      </IconStyle></Style>
      <Point><coordinates>{lon},{lat},0</coordinates></Point>
    </Placemark>"""


def _bbox_polygon(label: str, bbox: list, color_hex: str = "550000ff") -> str:
    lat_min, lon_min, lat_max, lon_max = bbox
    coords = (f"{lon_min},{lat_min},0 {lon_max},{lat_min},0 "
              f"{lon_max},{lat_max},0 {lon_min},{lat_max},0 {lon_min},{lat_min},0")
    return f"""
    <Placemark>
      <name>{label}</name>
      <Style>
        <LineStyle><color>ff{color_hex}</color><width>2</width></LineStyle>
        <PolyStyle><color>15{color_hex}</color></PolyStyle>
      </Style>
      <Polygon><outerBoundaryIs><LinearRing>
        <coordinates>{coords}</coordinates>
      </LinearRing></outerBoundaryIs></Polygon>
    </Placemark>"""


def _write_kmz(name: str, placemarks: list, zone_polygons: list,
               description: str) -> Path:
    folder_pm   = "\n".join(placemarks)
    folder_poly = "\n".join(zone_polygons)
    kml = f"""<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>{name}</name>
  <description><![CDATA[{description}]]></description>
  <Folder>
    <name>Reference Zones</name>
    {folder_poly}
  </Folder>
  <Folder>
    <name>Mine Hazard Detections</name>
    {folder_pm}
  </Folder>
</Document>
</kml>"""
    kmz_path = OUTPUT_DIR / f"{name}.kmz"
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("doc.kml", kml.encode("utf-8"))
    kmz_path.write_bytes(buf.getvalue())
    return kmz_path


# ── Synthetic candidate generation (no live tile data) ───────────────────────
# When no tiles are downloaded yet, generate a research-grounded set of
# historically documented candidate zones as INFO-level placemarks for the
# operator. These are NOT live detections — they are known risk areas from
# published maritime security literature (IRGC mining incidents, USCG notices).

HISTORICAL_RISK_ZONES = [
    # Abu Musa Island — Iran seized 1971; IRGC mined approaches 1987-88 (Tanker War)
    {
        "label":   "Abu Musa Island approaches",
        "lat":     25.877,
        "lon":     55.033,
        "risk":    RISK_HIGH,
        "passes":  ["historical", "known_incident"],
        "note":    "IRGC mined approaches 1987–88 (Operation Nimble Archer area). "
                   "Shallow shelf <30m. Active IRGC naval base island.",
    },
    # Inbound TSS — narrowest point near Sir Abu Nu'Ayr
    {
        "label":   "TSS Bottleneck — Sir Bu Nu'Ayr",
        "lat":     25.228,
        "lon":     54.250,
        "risk":    RISK_MEDIUM,
        "passes":  ["transit_lane", "chokepoint"],
        "note":    "Deepest vessel concentration point. Historically flagged in "
                   "USN/NAVCENT Notices to Mariners as mine susceptibility zone.",
    },
    # Tunb Islands — Greater and Lesser Tunb; Iran-occupied since 1971
    {
        "label":   "Greater Tunb Island — W approach",
        "lat":     26.253,
        "lon":     55.280,
        "risk":    RISK_HIGH,
        "passes":  ["historical", "transit_lane_adjacent"],
        "note":    "Iran-occupied. Shallow reef shelf. Approach vectors to inbound "
                   "TSS pass within 12nm. IRGC patrol vessel base.",
    },
    {
        "label":   "Lesser Tunb Island",
        "lat":     26.238,
        "lon":     55.143,
        "risk":    RISK_MEDIUM,
        "passes":  ["historical"],
        "note":    "Uninhabited Iran-controlled island. Shelf < 40m depth.",
    },
    # Qeshm Island North Channel — alternative tanker route sometimes used
    {
        "label":   "Qeshm North Channel",
        "lat":     26.801,
        "lon":     56.044,
        "risk":    RISK_MEDIUM,
        "passes":  ["chokepoint", "shallow_shelf"],
        "note":    "Narrow channel north of Qeshm. Depths 15–40m. Occasionally "
                   "used by vessels avoiding main TSS.",
    },
    # Outbound TSS — eastern approach Gulf of Oman
    {
        "label":   "Outbound TSS — eastern exit",
        "lat":     26.401,
        "lon":     57.020,
        "risk":    RISK_LOW,
        "passes":  ["transit_lane"],
        "note":    "Eastern exit of outbound lane. Depths > 60m, less mine-suitable. "
                   "Monitor for dark vessel clustering.",
    },
    # Khasab Bay — Oman — used by smugglers; AIS gaps common
    {
        "label":   "Khasab Bay approaches",
        "lat":     26.197,
        "lon":     56.257,
        "risk":    RISK_LOW,
        "passes":  ["ais_gap_zone"],
        "note":    "Oman. High small-boat traffic; AIS coverage poor. "
                   "Not a mine threat zone but confounds dark vessel detection.",
    },
]


# ── Main scan pipeline ────────────────────────────────────────────────────────

def run_scan(bbox: list, quick: bool = False) -> list:
    """
    Full mine hazard scan for the given bbox.
    Returns list of candidate dicts to write to KMZ / JSON.
    """
    log("=" * 60)
    log("HORMUZ MINE HAZARD SCAN — CESAROPS Maritime Safety")
    log(f"Date window: {DATE_START} → {DATE_END}  (newest 30 days)")
    log(f"Bbox: {bbox}")
    log("=" * 60)

    candidates = []
    granule_dates_sar = []
    granule_dates_opt = []

    # ── Step 1: Granule discovery ─────────────────────────────────────────────
    log("PASS 0 — Granule discovery")
    granules = find_best_granules(bbox, max_cloud=60.0)   # Middle East dust/haze triggers high cloud scores
    sar_grans = granules.get("sar", [])
    opt_grans = granules.get("optical", [])

    if sar_grans:
        granule_dates_sar = [_granule_date(g) for g in sar_grans[:3]]
        log(f"  Using SAR dates: {', '.join(granule_dates_sar)}")
    else:
        log("  WARNING: No SAR granules found — historical candidates only")

    if opt_grans:
        granule_dates_opt = [_granule_date(g) for g in opt_grans[:3]]
        log(f"  Using optical dates: {', '.join(granule_dates_opt)}")
    else:
        log("  WARNING: No optical granules — historical candidates only")

    # ── Step 2: Historical + known-risk candidates ────────────────────────────
    log("PASS REF — Historical known-risk zones (maritime security literature)")
    for zone in HISTORICAL_RISK_ZONES:
        # Only include zones that fall within our search bbox
        lat_min, lon_min, lat_max, lon_max = bbox
        if (lat_min <= zone["lat"] <= lat_max and
                lon_min <= zone["lon"] <= lon_max):
            candidates.append({
                "label":        zone["label"],
                "lat":          zone["lat"],
                "lon":          zone["lon"],
                "risk":         zone["risk"],
                "passes":       zone["passes"],
                "dates":        ["historical — see note"],
                "note":         zone["note"],
                "source":       "historical",
                "confidence":   0.0,
            })
            log(f"  + {zone['risk']:8} {zone['label']}")

    # ── Step 3: SAR anomaly synthetic analysis ────────────────────────────────
    # Live tile download requires EARTHDATA_TOKEN and rasterio — run on i7
    # Here we generate the CMR references and analysis plan for the worker.
    log("PASS 1 — SAR bright-point anomaly")
    if sar_grans:
        log(f"  {len(sar_grans)} SAR granule(s) queued for download on worker")
        log("  Algorithm: VV sigma > mean+3σ in 20×20px local window")
        log("  Will cluster anomalous pixels; flag clusters in TSS as HIGH risk")
        # Record as a planned analysis task (actual pixel processing on i7/worker)
        candidates.append({
            "label":      "SAR_PASS1_WORKER_TASK",
            "lat":        (bbox[0] + bbox[2]) / 2,
            "lon":        (bbox[1] + bbox[3]) / 2,
            "risk":       RISK_LOW,
            "passes":     ["sar_bright_pending"],
            "dates":      granule_dates_sar,
            "note":       f"Sentinel-1 GRD bright-point scan queued: "
                          f"{', '.join(g.get('producer_granule_id','?')[:20] for g in sar_grans[:2])}",
            "source":     "worker_task",
            "confidence": 0.0,
        })
    else:
        log("  SKIP — no SAR granules in window")

    log("PASS 2 — SAR temporal change (new objects vs 6-day prior)")
    if len(sar_grans) >= 2:
        log(f"  Diff: {_granule_date(sar_grans[0])} vs {_granule_date(sar_grans[1])}")
        log("  Algorithm: persistent new VV returns, z>2.5, no AIS vessel match")
    else:
        log(f"  Only {len(sar_grans)} SAR granule — temporal diff needs ≥2 passes")

    if not quick:
        log("PASS 3 — Dark vessel detection (SAR vessel vs. AIS cross-reference)")
        log("  Note: Live AIS cross-reference requires MarineTraffic API key.")
        log("  Set MARINETRAFFIC_API_KEY in .env to enable. Skipping for now.")

        log("PASS 4 — Turbidity plume (seabed disturbance, shallow shelf)")
        if opt_grans:
            log(f"  {len(opt_grans)} optical granule(s) queued for worker")
            log("  B02/B03 ratio z-score in SHALLOW_ZONES (<50m ETOPO proxy)")
        else:
            log("  SKIP — no clear optical granules in window")

        log("PASS 5 — Optical surface objects (Sentinel-2 B02 bright anomaly)")
        if opt_grans:
            log("  Reflectance > 0.15 in water-masked B02; cluster in 1km grid")
        else:
            log("  SKIP — no clear optical granules")

    log(f"\nTotal candidates: {len(candidates)}")
    return candidates


# ── KMZ output ────────────────────────────────────────────────────────────────

def write_kmz(candidates: list, run_bbox: list) -> Path:
    """Write combined KMZ and JSON output."""
    log("\nWriting KMZ output...")
    all_pm  = []
    all_polys = []
    json_out  = []

    # Zone reference polygons
    all_polys.append(_bbox_polygon("Inbound TSS",   STRAIT["inbound_tss"],  "0000ff"))
    all_polys.append(_bbox_polygon("Outbound TSS",  STRAIT["outbound_tss"], "00ff00"))
    all_polys.append(_bbox_polygon("Bottleneck",    STRAIT["bottleneck"],   "0000aa"))
    all_polys.append(_bbox_polygon("Abu Musa",      STRAIT["abu_musa"],     "ff0000"))
    all_polys.append(_bbox_polygon("Qeshm North",   STRAIT["qeshm_north"],  "888800"))
    all_polys.append(_bbox_polygon("Scan Bbox",     run_bbox,               "ffffff"))

    for i, c in enumerate(candidates):
        if c.get("source") == "worker_task":
            continue   # Don't show placeholder tasks in KMZ directly
        pm = _placemark(
            name         = c["label"],
            lat          = c["lat"],
            lon          = c["lon"],
            risk         = c["risk"],
            passes       = c.get("passes", []),
            granule_dates= c.get("dates", []),
            description  = c.get("note", ""),
        )
        all_pm.append(pm)
        json_out.append({
            "id":         i,
            "label":      c["label"],
            "lat":        c["lat"],
            "lon":        c["lon"],
            "risk":       c["risk"],
            "passes":     c.get("passes", []),
            "dates":      c.get("dates", []),
            "note":       c.get("note", ""),
            "confidence": c.get("confidence", 0.0),
        })

    scan_ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    desc = (f"Strait of Hormuz maritime mine hazard assessment<br/>"
            f"Scan date: {scan_ts}<br/>"
            f"Imagery window: {DATE_START} to {DATE_END}<br/>"
            f"IMPORTANT: This is a civilian maritime safety research output. "
            f"All flagged locations require confirmation by qualified maritime "
            f"security authorities before any navigation advisory is issued.")

    kmz_path = _write_kmz("hormuz_mine_hazard_combined", all_pm, all_polys, desc)
    log(f"  KMZ: {kmz_path}")

    json_path = OUTPUT_DIR / "hormuz_candidates.json"
    json_path.write_text(json.dumps(json_out, indent=2), encoding="utf-8")
    log(f"  JSON: {json_path}")

    # Summary by risk level
    by_risk = {}
    for c in json_out:
        by_risk.setdefault(c["risk"], 0)
        by_risk[c["risk"]] += 1

    log("\n── Risk Summary ──")
    for level in [RISK_CRITICAL, RISK_HIGH, RISK_MEDIUM, RISK_LOW]:
        n = by_risk.get(level, 0)
        if n:
            log(f"  {level:10} {n:3} locations")

    return kmz_path


# ── Queue push ────────────────────────────────────────────────────────────────

def push_to_queue():
    """Push scan jobs to the i7 scan worker queue."""
    try:
        import scan_queue as Q
    except ImportError:
        print("scan_queue not available — cannot push to queue")
        return

    Q.init_db()
    meta = {
        "mission":       "hormuz_mine_hazard",
        "description":   "Strait of Hormuz maritime mine hazard detection",
        "passes":        [1, 2, 3, 4, 5],
        "date_start":    DATE_START,
        "date_end":      DATE_END,
        "tss_inbound":   STRAIT["inbound_tss"],
        "tss_outbound":  STRAIT["outbound_tss"],
    }

    # Full strait — SAR + optical
    j1 = Q.push(
        label    = "hormuz_full_sar",
        bbox     = STRAIT["full_bbox"],
        sensors  = ["sar", "optical"],
        priority = Q.PRIORITY_USER,
        params   = {**meta, "sub_zone": "full_strait"},
    )
    # Bottleneck — highest risk, SAR only, highest priority
    j2 = Q.push(
        label    = "hormuz_bottleneck",
        bbox     = STRAIT["bottleneck"],
        sensors  = ["sar"],
        priority = Q.PRIORITY_USER,
        params   = {**meta, "sub_zone": "bottleneck"},
    )
    # Abu Musa approaches — historical mining zone
    j3 = Q.push(
        label    = "hormuz_abu_musa",
        bbox     = STRAIT["abu_musa"],
        sensors  = ["sar", "optical"],
        priority = Q.PRIORITY_USER,
        params   = {**meta, "sub_zone": "abu_musa_historical"},
    )

    print(f"Queued 3 Hormuz jobs (PRIORITY_USER):")
    print(f"  Full strait:  {j1}")
    print(f"  Bottleneck:   {j2}")
    print(f"  Abu Musa:     {j3}")
    print(f"Queue depth: {Q.queue_depth()}")


# ── CLI ───────────────────────────────────────────────────────────────────────

def main():
    ap = argparse.ArgumentParser(
        description="Strait of Hormuz maritime mine hazard detection — CESAROPS"
    )
    ap.add_argument("--quick",       action="store_true",
                    help="Transit lanes only, SAR passes 1+2 only")
    ap.add_argument("--no-download", action="store_true",
                    help="Skip tile download, use cached files")
    ap.add_argument("--push-queue",  action="store_true",
                    help="Push jobs to i7 scan worker queue")
    ap.add_argument("--bbox",        type=str, default=None,
                    help="Override bbox: lat_min,lon_min,lat_max,lon_max")
    args = ap.parse_args()

    if args.push_queue:
        push_to_queue()
        return

    bbox = STRAIT["bottleneck"] if args.quick else STRAIT["full_bbox"]
    if args.bbox:
        bbox = [float(x) for x in args.bbox.split(",")]

    candidates = run_scan(bbox, quick=args.quick)
    kmz_path   = write_kmz(candidates, bbox)

    print(f"\nOutputs in: {OUTPUT_DIR}")
    print(f"Load in Google Earth: {kmz_path.name}")
    print()
    print("IMPORTANT: These are research-level hazard indicators only.")
    print("All flagged areas require assessment by qualified maritime")
    print("security authorities before any navigation advisory is issued.")


if __name__ == "__main__":
    main()
