#!/usr/bin/env python3
"""
CESAROPS Pipeline KMZ -- WreckHunter 2000 / CESAROPS Platform
Exports all active missions + Great Lakes wreck detections to a single KMZ.

Structure:
  SONAR / BAG -- Great Lakes (NOAA nan_hole redactions)
    Lake Erie / Michigan / Huron / Superior / Ontario / Other
      High (>=0.95) / Standard (0.80-0.94)

  MISSIONS -- Satellite & Multi-Sensor Scans
    <Mission Name> [date_start -> date_end]  (one folder per mission campaign)
      <job label / sub-zone> [STATUS]        (one sub-folder per scan job)
        -- DONE: actual detection points from result JSON
        -- QUEUED/RUNNING/FAILED: status pin at bbox center

  GREAT LAKES -- Satellite Wreck Detection
    Lake Erie / Michigan / Huron / Superior / Ontario / Other
      High (>=0.80) / Moderate (0.60-0.79)

  CROSS-CONFIRMED (SONAR + Satellite agree within 1500 ft)
    Lake folders

MAG/Swayze data deliberately excluded. Swayze lat/lon is ML-estimated from
text descriptions -- kept in wrecks.db for identity lookup only.

Usage:
  python scripts/export_pipeline_kmz.py [--min-conf 0.0] [--out path/to.kmz]
"""

import argparse
import json
import math
import sqlite3
import sys
from pathlib import Path

import simplekml

# -- Paths ------------------------------------------------------------------
HERE     = Path(__file__).resolve().parent.parent
DB_PATH  = HERE / "db" / "wrecks.db"
QUEUE_DB = HERE / "db" / "scan_queue.db"
OUT_DIR  = HERE / "outputs"
OUT_KMZ  = OUT_DIR / "pipeline_anomalies.kmz"

# -- Mission display name mapping -------------------------------------------
MISSION_KEY_DISPLAY = {
    "hormuz_mine_hazard": "Strait of Hormuz -- Mine Hazard Scan",
    "canopy_sar_test":    "Alaska -- Canopy SAR Survey",
    "nwa_maritime":       "NWA Maritime Survey 2501",
    "nome_cessna":        "Nome -- Cessna SAR",
    "great_lakes_wreck":  "Great Lakes Wreck Detection",
}

# Sub-zone -> human label
SUBZONE_LABEL = {
    "full_strait":         "Full Strait",
    "bottleneck":          "Hormuz Bottleneck",
    "abu_musa_historical": "Abu Musa Island (Historical)",
}

# Job label -> friendly name
JOB_LABEL_DISPLAY = {
    "NWA_2501_primary":    "Primary Coverage",
    "NWA_2501_extended":   "Extended Coverage",
    "nome_cessna_primary": "Primary Grid",
    "nome_cessna_shore":   "Shoreline Grid",
    "nome_cessna_norton":  "Norton Sound",
    "alaska_canopy_areaA": "Area A -- Interior Alaska Taiga",
    "alaska_canopy_areaB": "Area B -- Denali Foothills",
    "alaska_canopy_areaC": "Area C -- Cook Inlet Coastal",
    "hormuz_full_sar":     "Full Strait SAR",
    "hormuz_bottleneck":   "Hormuz Bottleneck SAR",
    "hormuz_abu_musa":     "Abu Musa Island SAR",
}

# -- Lake bboxes ------------------------------------------------------------
LAKES = [
    ("Lake Ontario",  43.0, 44.5, -79.8, -76.0),
    ("Lake Erie",     41.3, 43.0, -83.5, -78.8),
    ("Lake Michigan", 41.5, 46.5, -88.0, -84.5),
    ("Lake Huron",    43.0, 46.8, -84.5, -79.5),
    ("Lake Superior", 46.0, 49.0, -92.5, -83.5),
]
LAKE_NAMES = [l[0] for l in LAKES] + ["Other"]


def lake_of(lat, lon):
    for name, lat_min, lat_max, lon_min, lon_max in LAKES:
        if lat_min <= lat <= lat_max and lon_min <= lon <= lon_max:
            return name
    return "Other"


def haversine_ft(lat1, lon1, lat2, lon2):
    R = 20_925_524
    dlat = math.radians(lat2 - lat1)
    dlon = math.radians(lon2 - lon1)
    a = (math.sin(dlat / 2) ** 2
         + math.cos(math.radians(lat1)) * math.cos(math.radians(lat2))
         * math.sin(dlon / 2) ** 2)
    return 2 * R * math.asin(math.sqrt(a))


CROSS_CONFIRM_RADIUS_FT = 1500

# -- Colors (ABGR) ----------------------------------------------------------
def _rgb(r, g, b, a=220):
    return simplekml.Color.rgb(r, g, b, a)

COL_BAG_HIGH = _rgb(0,   255, 220)   # cyan
COL_BAG_STD  = _rgb(255, 165,   0)   # orange
COL_SAT_HIGH = _rgb(0,   255,  80)   # bright green
COL_SAT_MED  = _rgb(180, 255,   0)   # yellow-green
COL_CROSS    = _rgb(255,   0,  60)   # red
COL_QUEUED   = _rgb(180, 180, 180)   # grey
COL_RUNNING  = _rgb(255, 230,   0)   # yellow
COL_FAILED   = _rgb(255,  60,  60)   # red-orange
COL_DONE     = _rgb(0,   200, 100)   # green

ICON_TARGET = "http://maps.google.com/mapfiles/kml/shapes/target.png"
ICON_DOT    = "http://maps.google.com/mapfiles/kml/shapes/placemark_circle.png"
ICON_CROSS  = "http://maps.google.com/mapfiles/kml/shapes/cross-hairs.png"
ICON_SQUARE = "http://maps.google.com/mapfiles/kml/shapes/square.png"
ICON_STAR   = "http://maps.google.com/mapfiles/kml/shapes/star.png"

STATUS_STYLE = {
    "QUEUED":    (COL_QUEUED,  ICON_DOT,    0.8),
    "RUNNING":   (COL_RUNNING, ICON_STAR,   1.2),
    "DONE":      (COL_DONE,    ICON_DOT,    1.0),
    "FAILED":    (COL_FAILED,  ICON_SQUARE, 0.9),
    "CANCELLED": (COL_QUEUED,  ICON_DOT,    0.5),
}


def _pt_style(pt, icon, color, scale=1.0):
    pt.style.iconstyle.icon.href = icon
    pt.style.iconstyle.color = color
    pt.style.iconstyle.scale = scale
    pt.style.labelstyle.scale = 0


def _make_lake_tier_folders(parent, tiers):
    out = {}
    for lake in LAKE_NAMES:
        lf = parent.newfolder(name=lake)
        out[lake] = {tier: lf.newfolder(name=tier) for tier in tiers}
    return out


# -- BAG / SONAR ------------------------------------------------------------
def _bag_desc(md):
    def row(label, key, unit=""):
        v = md.get(key)
        if v is not None and str(v).strip() not in ("", "None", "nan", "0.0"):
            return f"<tr><td><b>{label}</b></td><td>{v}{unit}</td></tr>"
        return ""
    body = (
        row("Pipeline",         "mask_type") +
        row("Candidate ID",     "candidate_id") +
        row("Survey",           "survey_id") +
        row("BAG File",         "bag_file") +
        row("Confidence",       "confidence") +
        row("Long side",        "long_side_ft",        " ft") +
        row("Short side",       "short_side_ft",        " ft") +
        row("Area",             "area_sq_ft",           " sq ft") +
        row("Cells",            "cell_count") +
        row("Depth (surr.)",    "surrounding_depth_ft", " ft") +
        row("Depth (restor.)",  "restored_depth_ft",    " ft") +
        row("Depth anomaly",    "depth_anomaly_ft",     " ft") +
        row("Resolution",       "resolution_ft",        " ft") +
        row("Filter run",       "filter_run_id") +
        row("Created",          "created_at") +
        row("Notes",            "notes")
    )
    return f"<html><body><table border='0' cellpadding='3'>{body}</table></body></html>"


def build_bag_layers(con, lake_folders, cross_set):
    cols  = [c[1] for c in con.execute("PRAGMA table_info(masking_candidates)")]
    rows  = con.execute(
        "SELECT * FROM masking_candidates WHERE center_lat IS NOT NULL"
    ).fetchall()
    total = 0
    for raw in rows:
        md   = dict(zip(cols, raw))
        conf = float(md.get("confidence") or 0)
        clat = md.get("center_lat")
        clon = md.get("center_lon")
        if clat is None or clon is None:
            continue

        lake   = lake_of(clat, clon)
        tier   = "High (>=0.95)" if conf >= 0.95 else "Standard (0.80-0.94)"
        color  = COL_BAG_HIGH if conf >= 0.95 else COL_BAG_STD
        parent = lake_folders[lake][tier]
        cid    = md.get("candidate_id", "UWC-?")
        desc   = _bag_desc(md)

        placed = False
        poly_json = md.get("polygon_json")
        if poly_json:
            try:
                pts    = json.loads(poly_json)
                coords = [(p[1], p[0]) for p in pts]
                if coords and coords[0] != coords[-1]:
                    coords.append(coords[0])
                pol = parent.newpolygon(name=cid, outerboundaryis=coords)
                pol.description = desc
                pol.style.polystyle.color = simplekml.Color.changealphaint(80, color)
                pol.style.linestyle.color = color
                pol.style.linestyle.width = 2
                placed = True
            except Exception:
                pass
        if not placed:
            pt = parent.newpoint(name=cid, coords=[(clon, clat)])
            pt.description = desc
            _pt_style(pt, ICON_TARGET, color, 1.0)

        cross_set["sonar"].append((clat, clon, cid, conf))
        total += 1
    return total


# -- Mission grouping -------------------------------------------------------
def _mission_key(label, params):
    m = params.get("mission", "")
    if m:
        return m
    pfx = label.split("_")[0].upper()
    if pfx == "NWA":
        return "nwa_maritime"
    if pfx == "NOME":
        return "nome_cessna"
    if pfx in ("MICHIGAN", "ERIE", "HURON", "SUPERIOR", "ONTARIO",
               "GREAT", "LAKES", "GL"):
        return "great_lakes_wreck"
    return label


def _mission_folder_name(key, jobs_in_mission):
    display = MISSION_KEY_DISPLAY.get(key, key.replace("_", " ").title())
    dates = set()
    for j in jobs_in_mission:
        d0 = j["_params"].get("date_start", "")
        d1 = j["_params"].get("date_end", "")
        if d0 or d1:
            dates.add((d0, d1))
    if dates:
        d0, d1 = sorted(dates)[0]
        if d0 and d1:
            display += f"  [{d0} to {d1}]"
        elif d0:
            display += f"  [from {d0}]"
    return display


def _job_subfolder_name(job):
    label  = job["label"]
    params = job["_params"]
    sz     = params.get("sub_zone", "")
    desc   = params.get("description", "")
    status = job["status"]

    if sz and sz in SUBZONE_LABEL:
        name = SUBZONE_LABEL[sz]
    elif sz:
        name = sz.replace("_", " ").title()
    elif label in JOB_LABEL_DISPLAY:
        name = JOB_LABEL_DISPLAY[label]
    elif desc:
        name = desc[:60]
    else:
        name = label
    return f"{name}  [{status}]"


def _job_desc(job):
    params  = job["_params"]
    sensors = json.dumps(job.get("_sensors") or [])
    body    = f"<tr><td><b>Job ID</b></td><td>{job['id']}</td></tr>"
    body   += f"<tr><td><b>Label</b></td><td>{job['label']}</td></tr>"
    body   += f"<tr><td><b>Status</b></td><td>{job['status']}</td></tr>"
    body   += f"<tr><td><b>Sensors</b></td><td>{sensors}</td></tr>"
    for k in ("mission", "description", "date_start", "date_end", "sub_zone", "passes"):
        v = params.get(k)
        if v:
            body += f"<tr><td><b>{k}</b></td><td>{str(v)[:120]}</td></tr>"
    if job.get("_bbox"):
        body += f"<tr><td><b>bbox</b></td><td>{job['_bbox']}</td></tr>"
    if job.get("error_msg"):
        body += f"<tr><td><b>Error</b></td><td>{str(job['error_msg'])[:300]}</td></tr>"
    if job.get("result_path"):
        body += f"<tr><td><b>Result</b></td><td>{job['result_path']}</td></tr>"
    return f"<html><body><table border='0' cellpadding='3'>{body}</table></body></html>"


def _bbox_center(bbox):
    try:
        b = (json.loads(bbox) if isinstance(bbox, str) else bbox) or []
        if len(b) == 4:
            return (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
    except Exception:
        pass
    return None, None


def _load_result_json(result_path):
    if not result_path:
        return []
    p = Path(result_path)
    for c in [p, p / "scan_results.json", p / "results.json",
              p / "anomalies.json", p / "wreck_candidates.json"]:
        if c.exists() and c.is_file():
            try:
                data  = json.loads(c.read_text())
                items = data if isinstance(data, list) else \
                        data.get("results", data.get("candidates", []))
                return items
            except Exception:
                pass
    return []


def _sat_conf(item):
    for key in ("confidence", "score", "anomaly_score", "probability"):
        v = item.get(key)
        if v is not None:
            return float(v)
    return 0.5


def _sat_desc(item):
    body = ""
    for k, v in item.items():
        if k.startswith("_") or v is None:
            continue
        body += f"<tr><td><b>{k}</b></td><td>{str(v)[:120]}</td></tr>"
    return f"<html><body><table border='0' cellpadding='3'>{body}</table></body></html>"


# -- Queue loader -----------------------------------------------------------
def load_queue_jobs():
    if not QUEUE_DB.exists():
        return []
    con  = sqlite3.connect(str(QUEUE_DB))
    cols = [c[1] for c in con.execute("PRAGMA table_info(scan_jobs)")]
    rows = con.execute(
        "SELECT * FROM scan_jobs ORDER BY priority DESC, created_at ASC"
    ).fetchall()
    con.close()
    jobs = []
    for raw in rows:
        d = dict(zip(cols, raw))
        d["_params"]  = json.loads(d.get("params")  or "{}")
        d["_sensors"] = json.loads(d.get("sensors") or "[]")
        d["_bbox"]    = json.loads(d.get("bbox")    or "[]")
        jobs.append(d)
    return jobs


# -- Mission layer builder --------------------------------------------------
def build_mission_layers(missions_root, gl_lake_folders, cross_set,
                         all_jobs, min_conf=0.0):
    # Group by mission key, de-dup by label (prefer DONE > RUNNING > QUEUED > FAILED > CANCELLED)
    STATUS_RANK = {"DONE": 0, "RUNNING": 1, "QUEUED": 2, "FAILED": 3, "CANCELLED": 4}
    groups: dict = {}
    for job in all_jobs:
        key = _mission_key(job["label"], job["_params"])
        groups.setdefault(key, {})
        lbl = job["label"]
        existing = groups[key].get(lbl)
        if existing is None or \
           STATUS_RANK.get(job["status"], 9) < STATUS_RANK.get(existing["status"], 9):
            groups[key][lbl] = job

    n_sat, n_jobs = 0, 0
    for key, label_map in groups.items():
        deduped = list(label_map.values())
        is_gl   = (key == "great_lakes_wreck")

        if not is_gl:
            mission_name   = _mission_folder_name(key, deduped)
            mission_folder = missions_root.newfolder(name=mission_name)

        for job in deduped:
            n_jobs  += 1
            status   = job["status"]
            clr, ico, scl = STATUS_STYLE.get(status, (COL_QUEUED, ICON_DOT, 0.8))

            if not is_gl:
                subfolder = mission_folder.newfolder(name=_job_subfolder_name(job))
                desc      = _job_desc(job)

            if status == "DONE" and job.get("result_path"):
                for item in _load_result_json(job["result_path"]):
                    lat = item.get("latitude") or item.get("lat") or item.get("center_lat")
                    lon = item.get("longitude") or item.get("lon") or item.get("center_lon")
                    if lat is None or lon is None:
                        continue
                    lat, lon = float(lat), float(lon)
                    conf = _sat_conf(item)
                    if conf < min_conf:
                        continue
                    label = (item.get("name") or item.get("candidate_id")
                             or f"{job['label']}-{n_sat}")
                    if is_gl:
                        lake   = lake_of(lat, lon)
                        tier   = "High (>=0.80)" if conf >= 0.80 else "Moderate (0.60-0.79)"
                        color  = COL_SAT_HIGH if conf >= 0.80 else COL_SAT_MED
                        pt     = gl_lake_folders[lake][tier].newpoint(
                            name=f"{label} [{conf:.0%}]", coords=[(lon, lat)])
                        cross_set["sat"].append((lat, lon, label, conf))
                    else:
                        color = COL_SAT_HIGH if conf >= 0.80 else COL_SAT_MED
                        pt    = subfolder.newpoint(
                            name=f"{label} [{conf:.0%}]", coords=[(lon, lat)])
                    pt.description = _sat_desc(item)
                    _pt_style(pt, ICON_DOT, color, 1.0)
                    n_sat += 1
            else:
                # Status pin at bbox center
                clat, clon = _bbox_center(job.get("_bbox") or [])
                if clat is None:
                    continue
                if is_gl:
                    lake = lake_of(clat, clon)
                    tier = "High (>=0.80)" if status == "DONE" else "Moderate (0.60-0.79)"
                    pt   = gl_lake_folders[lake][tier].newpoint(
                        name=_job_subfolder_name(job), coords=[(clon, clat)])
                    pt.description = _job_desc(job)
                else:
                    pt = subfolder.newpoint(
                        name=f"[{status}]  bbox center", coords=[(clon, clat)])
                    pt.description = _job_desc(job)
                _pt_style(pt, ico, clr, scl)

    return n_sat, n_jobs


# -- Cross-confirm ----------------------------------------------------------
def build_cross_layer(cross_folder, cross_set):
    total = 0
    for lat1, lon1, lbl1, c1 in cross_set["sonar"]:
        for lat2, lon2, lbl2, c2 in cross_set["sat"]:
            if haversine_ft(lat1, lon1, lat2, lon2) <= CROSS_CONFIRM_RADIUS_FT:
                lake   = lake_of(lat1, lon1)
                parent = cross_folder[lake]["BAG + Satellite"]
                avg    = (c1 + c2) / 2
                label  = f"XCONF-{total+1:03d} [{avg:.0%}]  {lat1:.4f},{lon1:.4f}"
                body   = ("<tr><td colspan='2'><b>Multiple pipelines "
                          "detect this target</b></td></tr>"
                          f"<tr><td><b>SONAR/BAG</b></td><td>{lbl1} ({c1:.0%})</td></tr>"
                          f"<tr><td><b>SATELLITE</b></td><td>{lbl2} ({c2:.0%})</td></tr>")
                pt     = parent.newpoint(name=label, coords=[(lon1, lat1)])
                pt.description = (
                    f"<html><body><table border='0' cellpadding='3'>"
                    f"{body}</table></body></html>")
                _pt_style(pt, ICON_CROSS, COL_CROSS, 1.6)
                total += 1
    return total


# -- Main ------------------------------------------------------------------
def main():
    ap = argparse.ArgumentParser(
        description="Export CESAROPS pipeline anomalies to KMZ")
    ap.add_argument("--min-conf", type=float, default=0.0)
    ap.add_argument("--out", default=str(OUT_KMZ))
    args = ap.parse_args()

    if not DB_PATH.exists():
        print(f"ERROR: wrecks.db not found at {DB_PATH}", file=sys.stderr)
        sys.exit(1)

    print(f"wrecks.db  : {DB_PATH}  ({DB_PATH.stat().st_size // 1024} KB)")
    if QUEUE_DB.exists():
        print(f"queue.db   : {QUEUE_DB}  ({QUEUE_DB.stat().st_size // 1024} KB)")
    print(f"Output     : {args.out}")
    print()

    wreck_con = sqlite3.connect(str(DB_PATH))
    kml       = simplekml.Kml(name="CESAROPS -- Pipeline Detections")
    cross_set = {"sonar": [], "sat": []}

    # 1. SONAR / BAG
    bag_root    = kml.newfolder(name="SONAR / BAG  (NOAA nan_hole redactions)")
    bag_folders = _make_lake_tier_folders(
        bag_root, ["High (>=0.95)", "Standard (0.80-0.94)"])
    n_bag       = build_bag_layers(wreck_con, bag_folders, cross_set)
    print(f"SONAR/BAG  : {n_bag} detections")

    # 2. Missions + Great Lakes satellite
    missions_root  = kml.newfolder(name="MISSIONS -- Satellite & Multi-Sensor Scans")
    gl_sat_root    = kml.newfolder(name="GREAT LAKES -- Satellite Wreck Detection")
    gl_sat_folders = _make_lake_tier_folders(
        gl_sat_root, ["High (>=0.80)", "Moderate (0.60-0.79)"])
    all_jobs       = load_queue_jobs()
    n_sat, n_jobs  = build_mission_layers(
        missions_root, gl_sat_folders, cross_set, all_jobs,
        min_conf=args.min_conf)
    print(f"Missions   : {n_jobs} scan jobs from queue.db")
    print(f"Satellite  : {n_sat} detection points (completed jobs)")

    # 3. Cross-confirmed
    xcon_root   = kml.newfolder(name="CROSS-CONFIRMED  (SONAR + Satellite agree)")
    xcon_folder = _make_lake_tier_folders(xcon_root, ["BAG + Satellite"])
    n_xcon      = build_cross_layer(xcon_folder, cross_set)
    print(f"Cross-conf : {n_xcon}")

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    kml.savekmz(str(out_path))
    print(f"\nSaved: {out_path}  ({out_path.stat().st_size // 1024} KB)")
    print("MAG/Swayze excluded -- estimated coords, identity reference only.")
    wreck_con.close()


if __name__ == "__main__":
    main()
