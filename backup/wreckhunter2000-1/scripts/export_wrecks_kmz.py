#!/usr/bin/env python3
"""
Export wrecks.db → Google Earth KMZ
Layers:
  1. Found / Confirmed wrecks  (green)
  2. Unknown / Not-yet-found   (yellow)
  3. Missing wrecks            (orange)
  4. Sonar candidates          (magenta polygons from BAG/NOAA files)
"""

import json
import sqlite3
import sys
from pathlib import Path
from typing import Optional

import simplekml

# ── paths ──────────────────────────────────────────────────────────────────
HERE = Path(__file__).resolve().parent.parent
DB_PATH = HERE / "db" / "wrecks.db"
OUT_DIR = HERE / "outputs"
OUT_KMZ = OUT_DIR / "wrecks_google_earth.kmz"

# ── icon URLs ──────────────────────────────────────────────────────────────
ICON_FOUND   = "http://maps.google.com/mapfiles/kml/paddle/grn-circle.png"
ICON_UNKNOWN = "http://maps.google.com/mapfiles/kml/paddle/ylw-circle.png"
ICON_MISSING = "http://maps.google.com/mapfiles/kml/paddle/orange-circle.png"
ICON_SONAR   = "http://maps.google.com/mapfiles/kml/paddle/red-stars.png"

# ── simplekml color (aabbggrr hex) ─────────────────────────────────────────
COL_FOUND   = simplekml.Color.changealphaint(200, simplekml.Color.green)
COL_UNKNOWN = simplekml.Color.changealphaint(200, simplekml.Color.yellow)
COL_MISSING = simplekml.Color.changealphaint(200, simplekml.Color.orange)
COL_SONAR_POLY = simplekml.Color.changealphaint(100, simplekml.Color.magenta)
COL_SONAR_LINE = simplekml.Color.changealphaint(220, simplekml.Color.magenta)

SCALE_FOUND   = 1.2
SCALE_UNKNOWN = 0.9
SCALE_MISSING = 1.1
SCALE_SONAR   = 1.0


def _fmt(v, unit: str = "") -> str:
    if v is None:
        return "—"
    return f"{v}{unit}"


def _feature_desc(row: dict) -> str:
    """Build an HTML description balloon for a features row."""
    parts = []
    for label, key, unit in [
        ("Type",        "feature_type",      ""),
        ("Date of Loss","date",              ""),
        ("Depth",       "depth",             ""),
        ("Hull",        "hull_material",     ""),
        ("Length",      "length_ft",         " ft"),
        ("Tonnage",     "tonnage",           ""),
        ("Cause",       "cause_of_loss",     ""),
        ("Cargo",       "cargo",             ""),
        ("Lives lost",  "lives_lost",        ""),
        ("Builder",     "builder",           ""),
        ("Year built",  "year_built",        ""),
        ("Owner",       "owner_at_loss",     ""),
        ("Lake/Region", "source",            ""),
        ("Found by",    "found_by",          ""),
        ("Found date",  "found_date",        ""),
        ("Found depth", "found_depth_m",     " m"),
        ("Mag weight",  "magnetic_weight",   ""),
        ("Train conf",  "training_confidence",""),
        ("Coord qual",  "coord_quality",     ""),
    ]:
        v = row.get(key)
        if v is not None and str(v).strip() not in ("", "None"):
            parts.append(f"<tr><td><b>{label}</b></td><td>{v}{unit}</td></tr>")

    narrative = row.get("description_narrative") or ""
    if narrative:
        parts.append(f"<tr><td colspan='2'><i>{narrative[:400]}</i></td></tr>")

    src_url = row.get("public_website") or ""
    if src_url:
        parts.append(f"<tr><td colspan='2'><a href='{src_url}'>🔗 More info</a></td></tr>")

    return (
        "<html><body><table border='0' cellpadding='3'>"
        + "".join(parts)
        + "</table></body></html>"
    )


def _sonar_desc(row: dict) -> str:
    parts = []
    for label, key, unit in [
        ("Candidate ID", "candidate_id",       ""),
        ("BAG file",     "bag_file",            ""),
        ("Survey ID",    "survey_id",           ""),
        ("Mask type",    "mask_type",           ""),
        ("Long side",    "long_side_ft",        " ft"),
        ("Short side",   "short_side_ft",       " ft"),
        ("Area",         "area_sq_ft",          " sq ft"),
        ("Cell count",   "cell_count",          ""),
        ("Surr depth",   "surrounding_depth_ft","ft"),
        ("Restored depth","restored_depth_ft", "ft"),
        ("Depth anomaly","depth_anomaly_ft",    " ft"),
        ("Confidence",   "confidence",          ""),
        ("Resolution",   "resolution_ft",       " ft"),
        ("Swayze match", "swayze_nearest",      ""),
        ("Swayze dist",  "swayze_dist_ft",      " ft"),
        ("Status",       "status",              ""),
        ("Created",      "created_at",          ""),
    ]:
        v = row.get(key)
        if v is not None and str(v).strip() not in ("", "None", "nan"):
            parts.append(f"<tr><td><b>{label}</b></td><td>{v}{unit}</td></tr>")

    notes = row.get("notes") or ""
    if notes:
        parts.append(f"<tr><td colspan='2'><i>{notes}</i></td></tr>")

    # Swayze matches
    matches_json = row.get("swayze_matches_json")
    if matches_json:
        try:
            matches = json.loads(matches_json)
            if matches:
                names = ", ".join(m.get("name", "?") for m in matches[:3])
                parts.append(f"<tr><td><b>Wreck matches</b></td><td>{names}</td></tr>")
        except Exception:
            pass

    return (
        "<html><body><table border='0' cellpadding='3'>"
        + "".join(parts)
        + "</table></body></html>"
    )


def add_feature_icon(pt, icon_href: str, color, scale: float = 1.0):
    pt.style.iconstyle.icon.href = icon_href
    pt.style.iconstyle.color = color
    pt.style.iconstyle.scale = scale
    pt.style.labelstyle.scale = 0.7


def build_kmz(db_path: Path, out_kmz: Path) -> None:
    con = sqlite3.connect(str(db_path))
    con.row_factory = sqlite3.Row

    kml = simplekml.Kml(name="WreckHunter 2000 — Great Lakes Shipwrecks")

    # ── Layer 1-3: Wrecks from features table ─────────────────────────────
    all_cols = [r[1] for r in con.execute("PRAGMA table_info(features)").fetchall()]

    sel = ", ".join(f"[{c}]" for c in all_cols)
    rows = con.execute(
        f"SELECT {sel} FROM features "
        f"WHERE latitude IS NOT NULL AND latitude != 0 AND longitude IS NOT NULL "
        f"AND NOT (latitude = 45.0 AND longitude = -83.0)"
    ).fetchall()
    print(f"Features with coords: {len(rows)}")

    # Deduplicate by (name, rounded lat/lon) to avoid exact-duplicate placemarks
    seen: set = set()
    deduped = []
    for r in rows:
        d = dict(r)
        key = (str(d.get("name", "")).lower().strip(),
               round(d.get("latitude", 0) or 0, 5),
               round(d.get("longitude", 0) or 0, 5))
        if key not in seen:
            seen.add(key)
            deduped.append(d)
    print(f"After dedup: {len(deduped)}")

    found_folder   = kml.newfolder(name=f"✅ Found / Confirmed ({sum(1 for d in deduped if d.get('found_status')=='found')})")
    unknown_folder = kml.newfolder(name=f"❓ Unknown / Not Yet Found ({sum(1 for d in deduped if d.get('found_status')!='found' and d.get('found_status')!='missing')})")
    missing_folder = kml.newfolder(name=f"⚠️ Missing ({sum(1 for d in deduped if d.get('found_status')=='missing')})")

    for d in deduped:
        lat = d.get("latitude")
        lon = d.get("longitude")
        name = str(d.get("name") or "Unknown").strip()
        status = d.get("found_status", "unknown") or "unknown"

        if status == "found":
            folder = found_folder
            icon   = ICON_FOUND
            color  = COL_FOUND
            scale  = SCALE_FOUND
        elif status == "missing":
            folder = missing_folder
            icon   = ICON_MISSING
            color  = COL_MISSING
            scale  = SCALE_MISSING
        else:
            folder = unknown_folder
            icon   = ICON_UNKNOWN
            color  = COL_UNKNOWN
            scale  = SCALE_UNKNOWN

        pt = folder.newpoint(name=name, coords=[(lon, lat)])
        pt.description = _feature_desc(d)
        add_feature_icon(pt, icon, color, scale)

        # Add altitude hint if depth known
        depth = d.get("depth")
        if depth is not None:
            pt.altitudemode = simplekml.AltitudeMode.clamptoground

    # ── Layer 4: Sonar candidates (polygons) ──────────────────────────────
    mc_cols = [r[1] for r in con.execute("PRAGMA table_info(masking_candidates)").fetchall()]
    mc_rows = con.execute("SELECT * FROM masking_candidates WHERE center_lat IS NOT NULL").fetchall()
    print(f"Sonar candidates: {len(mc_rows)}")

    sonar_folder = kml.newfolder(name=f"🔊 Sonar Candidates — BAG/NOAA ({len(mc_rows)})")

    for mr in mc_rows:
        md = dict(zip(mc_cols, mr))
        cid  = md.get("candidate_id", "UWC-?")
        dname = md.get("display_name", cid)
        clat  = md.get("center_lat")
        clon  = md.get("center_lon")
        conf  = md.get("confidence", 0) or 0

        poly_json = md.get("polygon_json")
        if poly_json:
            try:
                pts = json.loads(poly_json)
                # polygon_json is [[lat, lon], ...] — convert to [(lon, lat), ...]
                coords = [(p[1], p[0]) for p in pts]
                if coords and coords[0] != coords[-1]:
                    coords.append(coords[0])  # close ring

                pol = sonar_folder.newpolygon(
                    name=f"{dname} (conf={conf:.0%})",
                    outerboundaryis=coords,
                )
                pol.description = _sonar_desc(md)
                pol.style.polystyle.color = COL_SONAR_POLY
                pol.style.linestyle.color = COL_SONAR_LINE
                pol.style.linestyle.width = 2
                continue
            except Exception as e:
                pass  # fall through to point

        # Fallback: point if polygon fails
        if clat and clon:
            pt = sonar_folder.newpoint(name=f"{dname} (conf={conf:.0%})", coords=[(clon, clat)])
            pt.description = _sonar_desc(md)
            add_feature_icon(pt, ICON_SONAR, COL_SONAR_LINE, SCALE_SONAR)

    con.close()

    # ── Write KMZ ──────────────────────────────────────────────────────────
    out_kmz.parent.mkdir(parents=True, exist_ok=True)
    kml.savekmz(str(out_kmz))
    size_kb = out_kmz.stat().st_size // 1024
    print(f"\n✅ Saved: {out_kmz}  ({size_kb} KB)")
    print(f"   Layers: Found={sum(1 for d in deduped if d.get('found_status')=='found')}"
          f"  Unknown={sum(1 for d in deduped if d.get('found_status') not in ('found','missing'))}"
          f"  Missing={sum(1 for d in deduped if d.get('found_status')=='missing')}"
          f"  Sonar={len(mc_rows)}")


if __name__ == "__main__":
    if not DB_PATH.exists():
        print(f"ERROR: DB not found at {DB_PATH}", file=sys.stderr)
        sys.exit(1)
    print(f"DB: {DB_PATH}  ({DB_PATH.stat().st_size // 1024} KB)")
    build_kmz(DB_PATH, OUT_KMZ)
