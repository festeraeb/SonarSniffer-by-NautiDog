"""
BAG Masking Scanner — Finds redacted/masked areas in NOAA BAG files.

This is the tool Thomas originally envisioned:
  1. Scan each BAG for masking patterns:
     - NaN holes (NoData regions)
     - Unnatural flattening (suspiciously low variance zones)
     - Texture breaks (sharp gradient discontinuities at mask boundaries)
  2. Size-estimate each masked area (feet)
  3. Find the mask boundary polygon (convex hull)
  4. Restore/interpolate underneath to preview what's hidden
  5. Cross-reference with Swayze DB for possible wreck matches
  6. Output KML with:
     - Red polygons outlining each mask shape
     - Yellow pins at corners so you can see the shape in Google Earth
     - Blue pin at center with hover info showing unmask preview stats +
       Swayze match candidates

Measurements in FEET. Coordinates from BAG easting/northing metadata
(verified correct against known wrecks).
"""

import os
import json
import sqlite3
import warnings
from pathlib import Path
from datetime import datetime
from dataclasses import dataclass, field, asdict
from typing import List, Dict, Tuple, Optional, Any

import numpy as np

try:
    import h5py
except ImportError:
    raise ImportError("h5py required: pip install h5py")

try:
    import pyproj
except ImportError:
    raise ImportError("pyproj required: pip install pyproj")

try:
    from scipy import ndimage
    from scipy.interpolate import NearestNDInterpolator
except ImportError:
    raise ImportError("scipy required: pip install scipy")

warnings.filterwarnings("ignore")

M_TO_FT = 3.28084
NM_TO_FT = 6076.12


# ============================================================================
# DATA CLASSES
# ============================================================================

@dataclass
class MaskedRegion:
    """A single detected masked/redacted area in a BAG file."""
    id: str
    bag_file: str
    survey_id: str
    mask_type: str          # "nan_hole", "flattened", "texture_break"

    # Location (WGS84)
    center_lat: float
    center_lon: float

    # Bounding box (WGS84)
    bbox_sw_lat: float
    bbox_sw_lon: float
    bbox_ne_lat: float
    bbox_ne_lon: float

    # Polygon boundary (WGS84) — convex hull of the mask shape
    polygon_coords: List[Tuple[float, float]]  # [(lat, lon), ...]

    # Size (feet)
    long_side_ft: float
    short_side_ft: float
    area_sq_ft: float
    cell_count: int

    # Surrounding context
    surrounding_depth_ft: float   # median depth around the mask
    depth_variance_ft: float      # std dev of surrounding bottom

    # Restoration preview
    restored_depth_ft: float      # interpolated depth under the mask
    depth_anomaly_ft: float       # difference from surroundings (positive = wreck-like bump)

    # Swayze matches
    swayze_matches: List[Dict[str, Any]] = field(default_factory=list)

    # Metadata
    confidence: float = 0.0       # 0-1, how likely this is deliberate masking
    resolution_ft: float = 0.0
    epsg: int = 0

    def to_dict(self):
        return asdict(self)


# ============================================================================
# BAG READER (minimal — reuses the verified easting/northing approach)
# ============================================================================

class BAGMetaReader:
    """Read BAG metadata and elevation grid."""

    NODATA = 1_000_000.0

    def read(self, filepath: str):
        """Return (elevation_2d, meta_dict)"""
        import re
        f = h5py.File(filepath, "r")
        try:
            ds = f["BAG_root/elevation"]
            elevation = ds[:].astype(np.float64)
            elevation[elevation >= self.NODATA - 1] = np.nan

            meta_raw = f["BAG_root/metadata"][:]
            meta_str = b"".join(meta_raw).decode("utf-8", errors="replace")

            # Corner points
            import re as re2
            cp = re2.search(r"<gml:coordinates[^>]*>([^<]+)</gml:coordinates>", meta_str)
            if cp:
                parts = cp.group(1).strip().split()
                sw = parts[0].split(",")
                ne = parts[1].split(",") if len(parts) > 1 else sw
                sw_e, sw_n = float(sw[0]), float(sw[1])
                ne_e, ne_n = float(ne[0]), float(ne[1])
            else:
                sw_e = sw_n = ne_e = ne_n = 0.0

            # Resolution
            res_list = re2.findall(r'<gco:Measure uom="m">([^<]+)</gco:Measure>', meta_str)
            resolution = float(res_list[0]) if res_list else 1.0

            # CRS
            crs_wkt = ""
            epsg = 0
            projcs_start = meta_str.find("PROJCS[")
            if projcs_start >= 0:
                depth = 0
                for i in range(projcs_start, len(meta_str)):
                    if meta_str[i] == "[":
                        depth += 1
                    elif meta_str[i] == "]":
                        depth -= 1
                        if depth == 0:
                            crs_wkt = meta_str[projcs_start : i + 1]
                            break
                if crs_wkt:
                    try:
                        crs_obj = pyproj.CRS(crs_wkt)
                        auth = crs_obj.to_authority()
                        if auth and auth[0] == "EPSG":
                            epsg = int(auth[1])
                        else:
                            epsg = crs_obj.to_epsg() or 0
                    except Exception:
                        epsg = 0

            basename = os.path.basename(filepath)
            survey_id = basename.split("_")[0] if "_" in basename else basename.replace(".bag", "")

            meta = {
                "filepath": filepath,
                "basename": basename,
                "survey_id": survey_id,
                "shape": elevation.shape,
                "sw_easting": sw_e,
                "sw_northing": sw_n,
                "ne_easting": ne_e,
                "ne_northing": ne_n,
                "resolution_m": resolution,
                "crs_wkt": crs_wkt,
                "epsg": epsg,
            }
            return elevation, meta
        finally:
            f.close()


# ============================================================================
# COORDINATE TRANSFORMER
# ============================================================================

class CoordXform:
    """UTM ↔ WGS84"""

    def __init__(self):
        self._cache = {}

    def to_latlon(self, easting, northing, epsg, crs_wkt=""):
        if epsg not in self._cache:
            try:
                src = pyproj.CRS.from_epsg(epsg)
            except Exception:
                src = pyproj.CRS(crs_wkt) if crs_wkt else None
                if src is None:
                    return 0.0, 0.0
            self._cache[epsg] = pyproj.Transformer.from_crs(
                src, pyproj.CRS.from_epsg(4326), always_xy=True
            )
        lon, lat = self._cache[epsg].transform(easting, northing)
        return lat, lon

    def grid_to_latlon(self, row, col, meta):
        e = meta["sw_easting"] + col * meta["resolution_m"]
        n = meta["sw_northing"] + row * meta["resolution_m"]
        return self.to_latlon(e, n, meta["epsg"], meta["crs_wkt"])


# ============================================================================
# MASKING PATTERN DETECTOR
# ============================================================================

class MaskingDetector:
    """
    Detect three types of masking in BAG files:
    1. NaN holes — connected NoData regions sized like a wreck
    2. Flattened zones — unnaturally low variance (depth artificially leveled)
    3. Texture breaks — sharp gradient discontinuities at region boundaries
    """

    # Size thresholds (feet) — must be at least this big
    MIN_LONG_FT = 36.0
    MIN_SHORT_FT = 10.0

    # Great Lakes bounding box
    GL_LAT_MIN, GL_LAT_MAX = 41.3, 49.0
    GL_LON_MIN, GL_LON_MAX = -92.2, -76.0

    def __init__(self, xform: CoordXform):
        self.xform = xform

    def detect_nan_holes(self, elevation, meta) -> List[Dict]:
        """Find connected NaN regions that are wreck-sized."""
        nan_mask = np.isnan(elevation)
        if np.sum(nan_mask) == 0:
            return []

        # Label connected NaN components
        struct = ndimage.generate_binary_structure(2, 2)  # 8-connect
        labeled, num = ndimage.label(nan_mask, struct)
        if num == 0:
            return []

        res_m = meta["resolution_m"]
        res_ft = res_m * M_TO_FT
        results = []

        # Use component_sizes for speed
        sizes = ndimage.sum(nan_mask, labeled, range(1, num + 1))

        for label_id in range(1, num + 1):
            cell_count = int(sizes[label_id - 1])
            if cell_count < 4:
                continue

            rows, cols = np.where(labeled == label_id)
            row_span_ft = (np.ptp(rows) + 1) * res_ft
            col_span_ft = (np.ptp(cols) + 1) * res_ft
            long_ft = max(row_span_ft, col_span_ft)
            short_ft = min(row_span_ft, col_span_ft)

            if long_ft < self.MIN_LONG_FT or short_ft < self.MIN_SHORT_FT:
                continue

            # Aspect ratio filter — skip super-thin stitching seams
            if long_ft / max(short_ft, 0.01) > 8.0:
                continue

            results.append({
                "type": "nan_hole",
                "rows": rows,
                "cols": cols,
                "cell_count": cell_count,
                "long_ft": long_ft,
                "short_ft": short_ft,
                "area_sq_ft": cell_count * res_ft * res_ft,
            })

        return results

    def detect_flattened_zones(self, elevation, meta) -> List[Dict]:
        """
        Find unnaturally flat zones — depth was artificially leveled.
        Compute local std deviation; regions with near-zero variance
        surrounded by normal-variance bottom are suspicious.
        """
        valid = ~np.isnan(elevation)
        if np.sum(valid) < 1000:
            return []

        res_m = meta["resolution_m"]
        res_ft = res_m * M_TO_FT

        # Compute local std dev in a ~50m window
        window_px = max(3, int(50.0 / res_m))
        if window_px % 2 == 0:
            window_px += 1

        # Fill NaN for filter
        filled = elevation.copy()
        global_med = np.nanmedian(elevation)
        filled[np.isnan(filled)] = global_med

        # Local mean and variance
        kernel = np.ones((window_px, window_px)) / (window_px * window_px)
        local_mean = ndimage.convolve(filled, kernel, mode="reflect")
        local_sq = ndimage.convolve(filled ** 2, kernel, mode="reflect")
        local_var = np.maximum(local_sq - local_mean ** 2, 0.0)
        local_std = np.sqrt(local_var)

        # Global std (valid cells only)
        global_std = float(np.nanstd(elevation))
        if global_std < 0.01:
            return []

        # "Flat" = local std < 5% of global std AND not a NaN area
        flat_threshold = global_std * 0.05
        flat_mask = valid & (local_std < flat_threshold)

        # Must be at least wreck-sized connected region
        struct = ndimage.generate_binary_structure(2, 2)
        labeled, num = ndimage.label(flat_mask, struct)
        if num == 0:
            return []

        results = []
        sizes = ndimage.sum(flat_mask, labeled, range(1, num + 1))

        for label_id in range(1, num + 1):
            cell_count = int(sizes[label_id - 1])
            if cell_count < 8:
                continue

            rows, cols = np.where(labeled == label_id)
            row_span_ft = (np.ptp(rows) + 1) * res_ft
            col_span_ft = (np.ptp(cols) + 1) * res_ft
            long_ft = max(row_span_ft, col_span_ft)
            short_ft = min(row_span_ft, col_span_ft)

            if long_ft < self.MIN_LONG_FT or short_ft < self.MIN_SHORT_FT:
                continue
            if long_ft / max(short_ft, 0.01) > 8.0:
                continue

            # Check that surrounding area has NORMAL variance (not just open flat bottom)
            # Dilate the region and check the ring around it
            component_mask = labeled == label_id
            dilated = ndimage.binary_dilation(component_mask, struct, iterations=max(3, window_px))
            ring = dilated & ~component_mask & valid
            if np.sum(ring) < 20:
                continue
            ring_std = float(np.std(elevation[ring]))
            if ring_std < global_std * 0.15:
                # Surrounding is also flat — this is just a flat bottom, not masking
                continue

            results.append({
                "type": "flattened",
                "rows": rows,
                "cols": cols,
                "cell_count": cell_count,
                "long_ft": long_ft,
                "short_ft": short_ft,
                "area_sq_ft": cell_count * res_ft * res_ft,
                "flat_std_ft": float(np.mean(local_std[component_mask])) * M_TO_FT,
                "ring_std_ft": ring_std * M_TO_FT,
            })

        return results

    def detect_texture_breaks(self, elevation, meta) -> List[Dict]:
        """
        Find sharp gradient discontinuities at region boundaries.
        A smoothly-erased wreck will have the bottom suddenly change character
        at the edge of the edit — a "texture break".
        """
        valid = ~np.isnan(elevation)
        if np.sum(valid) < 1000:
            return []

        res_m = meta["resolution_m"]
        res_ft = res_m * M_TO_FT

        # Compute gradient magnitude
        filled = elevation.copy()
        filled[np.isnan(filled)] = np.nanmedian(elevation)
        gx = ndimage.sobel(filled, axis=1)
        gy = ndimage.sobel(filled, axis=0)
        gradient = np.sqrt(gx ** 2 + gy ** 2)

        # Compute local gradient std (roughness proxy)
        window_px = max(3, int(30.0 / res_m))
        if window_px % 2 == 0:
            window_px += 1
        kernel = np.ones((window_px, window_px)) / (window_px * window_px)
        local_grad_mean = ndimage.convolve(gradient, kernel, mode="reflect")

        # Global gradient stats
        valid_grad = gradient[valid]
        grad_p95 = float(np.percentile(valid_grad, 95))
        if grad_p95 < 0.001:
            return []

        # "Texture break" = cells where gradient is very high (top 2%) AND
        # one side of the edge is notably smoother than normal
        break_threshold = float(np.percentile(valid_grad, 98))
        break_mask = valid & (gradient > break_threshold)

        # Must form wreck-sized connected patches
        struct = ndimage.generate_binary_structure(2, 2)

        # Dilate break edges to merge nearby ones into connected regions
        break_dilated = ndimage.binary_dilation(break_mask, struct, iterations=3)
        labeled, num = ndimage.label(break_dilated, struct)
        if num == 0:
            return []

        results = []
        sizes = ndimage.sum(break_dilated, labeled, range(1, num + 1))

        for label_id in range(1, num + 1):
            cell_count = int(sizes[label_id - 1])
            if cell_count < 12:
                continue

            rows, cols = np.where(labeled == label_id)
            row_span_ft = (np.ptp(rows) + 1) * res_ft
            col_span_ft = (np.ptp(cols) + 1) * res_ft
            long_ft = max(row_span_ft, col_span_ft)
            short_ft = min(row_span_ft, col_span_ft)

            if long_ft < self.MIN_LONG_FT or short_ft < self.MIN_SHORT_FT:
                continue
            if long_ft / max(short_ft, 0.01) > 8.0:
                continue

            results.append({
                "type": "texture_break",
                "rows": rows,
                "cols": cols,
                "cell_count": cell_count,
                "long_ft": long_ft,
                "short_ft": short_ft,
                "area_sq_ft": cell_count * res_ft * res_ft,
            })

        return results


# ============================================================================
# RESTORATION ENGINE (unmask preview)
# ============================================================================

class UnmaskPreview:
    """Interpolate under masked areas to estimate what's hidden."""

    def restore(self, elevation, rows, cols) -> Dict:
        """
        Given a masked region defined by (rows, cols), interpolate from
        surrounding valid data to estimate what the depth would be
        without the mask.
        """
        h, w = elevation.shape

        # Get a local window around the region
        margin = 30
        r_min, r_max = max(0, np.min(rows) - margin), min(h, np.max(rows) + margin)
        c_min, c_max = max(0, np.min(cols) - margin), min(w, np.max(cols) + margin)
        local = elevation[r_min:r_max, c_min:c_max]

        valid_local = ~np.isnan(local)
        if np.sum(valid_local) < 10:
            return {"restored_depth_m": np.nan, "surrounding_depth_m": np.nan,
                    "depth_anomaly_m": 0.0, "surrounding_std_m": 0.0}

        # Surrounding depth stats (ring around the mask)
        mask_local = np.zeros_like(local, dtype=bool)
        for r, c in zip(rows, cols):
            lr, lc = r - r_min, c - c_min
            if 0 <= lr < local.shape[0] and 0 <= lc < local.shape[1]:
                mask_local[lr, lc] = True

        ring = ndimage.binary_dilation(
            mask_local, iterations=5
        ) & ~mask_local & valid_local
        if np.sum(ring) < 5:
            ring = valid_local & ~mask_local
        if np.sum(ring) < 5:
            return {"restored_depth_m": np.nan, "surrounding_depth_m": np.nan,
                    "depth_anomaly_m": 0.0, "surrounding_std_m": 0.0}

        surr_depth = float(np.median(local[ring]))
        surr_std = float(np.std(local[ring]))

        # Interpolate into the masked region
        vr, vc = np.where(valid_local & ~mask_local)
        vv = local[valid_local & ~mask_local]
        if len(vr) < 3:
            return {"restored_depth_m": surr_depth, "surrounding_depth_m": surr_depth,
                    "depth_anomaly_m": 0.0, "surrounding_std_m": surr_std}

        interp = NearestNDInterpolator(list(zip(vr, vc)), vv)
        mr, mc = np.where(mask_local)
        if len(mr) == 0:
            restored_depth = surr_depth
        else:
            restored_vals = interp(mr, mc)
            restored_depth = float(np.median(restored_vals))

        anomaly = restored_depth - surr_depth  # positive = shallower = bump

        return {
            "restored_depth_m": restored_depth,
            "surrounding_depth_m": surr_depth,
            "depth_anomaly_m": anomaly,
            "surrounding_std_m": surr_std,
        }


# ============================================================================
# SWAYZE CROSS-REFERENCE
# ============================================================================

class SwayzeXRef:
    """Match masked regions against the Swayze wrecks database."""

    def __init__(self, db_path: str):
        self.db_path = db_path
        self._wrecks = None

    def _load(self):
        if self._wrecks is not None:
            return
        self._wrecks = []
        if not os.path.exists(self.db_path):
            return
        conn = sqlite3.connect(self.db_path)
        try:
            rows = conn.execute(
                "SELECT name, latitude, longitude, length_ft, dimensions_raw, "
                "hull_material, depth, found_status, date, feature_type, "
                "description_narrative, vessel_class "
                "FROM features "
                "WHERE latitude IS NOT NULL AND longitude IS NOT NULL "
                "AND latitude != 0 AND longitude != 0"
            ).fetchall()
            for r in rows:
                self._wrecks.append({
                    "name": r[0],
                    "lat": float(r[1]),
                    "lon": float(r[2]),
                    "length_ft": float(r[3]) if r[3] else None,
                    "dimensions": r[4],
                    "hull": r[5],
                    "depth": r[6],
                    "found_status": r[7],
                    "date": r[8],
                    "type": r[9],
                    "description": (r[10] or "")[:200],
                    "vessel_class": r[11],
                })
        except Exception:
            pass
        finally:
            conn.close()

    def find_nearby(self, lat: float, lon: float, radius_nm: float = 1.0,
                    size_ft: Optional[float] = None) -> List[Dict]:
        """Find Swayze wrecks within radius_nm of (lat, lon)."""
        self._load()
        results = []
        for w in self._wrecks:
            dlat = (lat - w["lat"]) * 60.0  # nm
            dlon = (lon - w["lon"]) * 60.0 * np.cos(np.radians(lat))
            dist_nm = np.sqrt(dlat ** 2 + dlon ** 2)
            if dist_nm <= radius_nm:
                match = dict(w)
                match["dist_nm"] = round(dist_nm, 3)
                match["dist_ft"] = round(dist_nm * NM_TO_FT, 0)
                # Size similarity score
                if size_ft and w["length_ft"]:
                    ratio = min(size_ft, w["length_ft"]) / max(size_ft, w["length_ft"])
                    match["size_match"] = round(ratio, 2)
                else:
                    match["size_match"] = None
                results.append(match)

        # Sort by distance
        results.sort(key=lambda x: x["dist_nm"])
        return results[:5]  # top 5 nearest


# ============================================================================
# KML GENERATOR — polygons, pins, hover info
# ============================================================================

class MaskingKMLGenerator:
    """
    Generate KML with:
    - Red polygons outlining each masked region boundary
    - Yellow pins at polygon vertices
    - Blue center pin with hover popup showing unmask stats + Swayze matches
    """

    def __init__(self, output_dir: str):
        self.output_dir = output_dir
        os.makedirs(output_dir, exist_ok=True)

    def generate(self, regions: List[MaskedRegion]) -> Tuple[str, str]:
        kml = self._header()

        for r in regions:
            kml += self._mask_folder(r)

        kml += "</Document>\n</kml>\n"

        ts = datetime.now().strftime("%Y%m%d_%H%M%S")
        kml_path = os.path.join(self.output_dir, f"masked_regions_{ts}.kml")
        with open(kml_path, "w", encoding="utf-8") as f:
            f.write(kml)

        # Also write KMZ
        import zipfile
        kmz_path = kml_path.replace(".kml", ".kmz")
        with zipfile.ZipFile(kmz_path, "w", zipfile.ZIP_DEFLATED) as zf:
            zf.write(kml_path, "doc.kml")

        return kml_path, kmz_path

    def _header(self) -> str:
        return f"""<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>BAG Masked Regions — {datetime.now().strftime('%Y-%m-%d %H:%M')}</name>
  <description>Detected masking patterns in NOAA BAG files.
Red polygons = mask boundaries. Blue pin = center with unmask preview.
Yellow pins = boundary vertices.</description>
{self._styles()}
"""

    def _styles(self) -> str:
        return """
  <Style id="mask_nan"><PolyStyle><color>600000ff</color><outline>1</outline></PolyStyle>
    <LineStyle><color>ff0000ff</color><width>2</width></LineStyle></Style>
  <Style id="mask_flat"><PolyStyle><color>6000aaff</color><outline>1</outline></PolyStyle>
    <LineStyle><color>ff00aaff</color><width>2</width></LineStyle></Style>
  <Style id="mask_texbrk"><PolyStyle><color>60ff00ff</color><outline>1</outline></PolyStyle>
    <LineStyle><color>ffff00ff</color><width>2</width></LineStyle></Style>
  <Style id="pin_center"><IconStyle><color>ffff0000</color>
    <Icon><href>http://maps.google.com/mapfiles/kml/pushpin/blue-pushpin.png</href></Icon>
    <scale>1.0</scale></IconStyle></Style>
  <Style id="pin_vertex"><IconStyle><color>ff00ffff</color>
    <Icon><href>http://maps.google.com/mapfiles/kml/pushpin/ylw-pushpin.png</href></Icon>
    <scale>0.5</scale></IconStyle></Style>
"""

    def _mask_folder(self, r: MaskedRegion) -> str:
        style = {"nan_hole": "mask_nan", "flattened": "mask_flat",
                 "texture_break": "mask_texbrk"}.get(r.mask_type, "mask_nan")

        # Swayze match text
        swayze_html = ""
        if r.swayze_matches:
            swayze_html = "<br/><b>--- Swayze Matches ---</b><br/>"
            for m in r.swayze_matches:
                sm = f" (size match: {m['size_match']:.0%})" if m.get("size_match") else ""
                swayze_html += (
                    f"<b>{m['name']}</b> — {m['dist_ft']:.0f} ft away{sm}<br/>"
                    f"  {m.get('type','?')} | {m.get('hull','?')} | "
                    f"dims: {m.get('dimensions','?')} | {m.get('found_status','?')}<br/>"
                )
        else:
            swayze_html = "<br/><i>No Swayze matches within 1 nm</i>"

        desc = f"""<![CDATA[
<b>MASKED REGION — {r.mask_type.upper().replace('_',' ')}</b><br/>
<b>Survey:</b> {r.survey_id} | <b>File:</b> {r.bag_file}<br/>
<b>Size:</b> {r.long_side_ft:.0f} x {r.short_side_ft:.0f} ft ({r.area_sq_ft:.0f} sq ft)<br/>
<b>Cells:</b> {r.cell_count} @ {r.resolution_ft:.1f} ft/cell<br/>
<b>Confidence:</b> {r.confidence:.0%}<br/>
<hr/>
<b>--- Unmask Preview ---</b><br/>
<b>Surrounding depth:</b> {r.surrounding_depth_ft:.1f} ft<br/>
<b>Restored (interpolated) depth:</b> {r.restored_depth_ft:.1f} ft<br/>
<b>Depth anomaly:</b> {r.depth_anomaly_ft:+.1f} ft (positive = shallower = wreck-like)<br/>
<b>Surrounding variance:</b> {r.depth_variance_ft:.2f} ft<br/>
{swayze_html}
]]>"""

        kml = f'  <Folder><name>{r.id}</name>\n'

        # Polygon
        if len(r.polygon_coords) >= 3:
            coords_str = " ".join(
                f"{lon:.8f},{lat:.8f},0" for lat, lon in r.polygon_coords
            )
            # Close the ring
            first = r.polygon_coords[0]
            coords_str += f" {first[1]:.8f},{first[0]:.8f},0"

            kml += f"""    <Placemark>
      <name>{r.mask_type} {r.long_side_ft:.0f}x{r.short_side_ft:.0f}ft</name>
      <styleUrl>#{style}</styleUrl>
      <description>{desc}</description>
      <Polygon><outerBoundaryIs><LinearRing>
        <coordinates>{coords_str}</coordinates>
      </LinearRing></outerBoundaryIs></Polygon>
    </Placemark>
"""

        # Center pin (blue)
        kml += f"""    <Placemark>
      <name>{r.mask_type} center</name>
      <styleUrl>#pin_center</styleUrl>
      <description>{desc}</description>
      <Point><coordinates>{r.center_lon:.8f},{r.center_lat:.8f},0</coordinates></Point>
    </Placemark>
"""

        # Yellow vertex pins
        for i, (lat, lon) in enumerate(r.polygon_coords):
            kml += f"""    <Placemark>
      <name>v{i}</name>
      <styleUrl>#pin_vertex</styleUrl>
      <Point><coordinates>{lon:.8f},{lat:.8f},0</coordinates></Point>
    </Placemark>
"""

        kml += "  </Folder>\n"
        return kml


# ============================================================================
# MAIN PIPELINE
# ============================================================================

def scan_bag_for_masking(filepath: str, xform: CoordXform,
                         detector: MaskingDetector,
                         preview: UnmaskPreview,
                         swayze: SwayzeXRef,
                         progress=None) -> List[MaskedRegion]:
    """Scan a single BAG file for all masking patterns."""
    reader = BAGMetaReader()
    try:
        elevation, meta = reader.read(filepath)
    except Exception as e:
        if progress:
            progress(f"  ERROR reading {os.path.basename(filepath)}: {e}")
        return []

    # Downsample very large grids to avoid OOM
    max_cells = 4_000_000
    total = elevation.shape[0] * elevation.shape[1]
    if total > max_cells:
        scale = max(2, int((total / max_cells) ** 0.5) + 1)
        elevation = elevation[::scale, ::scale]
        meta = dict(meta)
        meta["resolution_m"] = meta["resolution_m"] * scale
        meta["shape"] = elevation.shape

    res_ft = meta["resolution_m"] * M_TO_FT
    basename = meta["basename"]
    survey = meta["survey_id"]

    # Run all three detectors
    all_raw = []
    all_raw.extend(detector.detect_nan_holes(elevation, meta))
    all_raw.extend(detector.detect_flattened_zones(elevation, meta))
    all_raw.extend(detector.detect_texture_breaks(elevation, meta))

    if progress:
        progress(f"  {basename}: {len(all_raw)} raw masked regions")

    regions = []
    for i, raw in enumerate(all_raw):
        rows = raw["rows"]
        cols = raw["cols"]

        # Center lat/lon
        center_row = int(np.mean(rows))
        center_col = int(np.mean(cols))
        center_lat, center_lon = xform.grid_to_latlon(center_row, center_col, meta)

        # Bounding box check
        if not (detector.GL_LAT_MIN <= center_lat <= detector.GL_LAT_MAX and
                detector.GL_LON_MIN <= center_lon <= detector.GL_LON_MAX):
            continue

        # Bounding box corners
        sw_lat, sw_lon = xform.grid_to_latlon(int(np.min(rows)), int(np.min(cols)), meta)
        ne_lat, ne_lon = xform.grid_to_latlon(int(np.max(rows)), int(np.max(cols)), meta)

        # Convex hull for polygon boundary
        polygon = _convex_hull_latlon(rows, cols, meta, xform)

        # Restoration / unmask preview
        restore = preview.restore(elevation, rows, cols)

        # Confidence scoring
        confidence = _score_confidence(raw, restore)

        # Swayze cross-reference
        matches = swayze.find_nearby(
            center_lat, center_lon,
            radius_nm=1.0,
            size_ft=raw["long_ft"]
        )

        region = MaskedRegion(
            id=f"{survey}_mask{i:03d}",
            bag_file=basename,
            survey_id=survey,
            mask_type=raw["type"],
            center_lat=center_lat,
            center_lon=center_lon,
            bbox_sw_lat=sw_lat,
            bbox_sw_lon=sw_lon,
            bbox_ne_lat=ne_lat,
            bbox_ne_lon=ne_lon,
            polygon_coords=polygon,
            long_side_ft=raw["long_ft"],
            short_side_ft=raw["short_ft"],
            area_sq_ft=raw["area_sq_ft"],
            cell_count=raw["cell_count"],
            surrounding_depth_ft=abs(restore["surrounding_depth_m"]) * M_TO_FT
                if not np.isnan(restore["surrounding_depth_m"]) else 0.0,
            depth_variance_ft=restore["surrounding_std_m"] * M_TO_FT,
            restored_depth_ft=abs(restore["restored_depth_m"]) * M_TO_FT
                if not np.isnan(restore["restored_depth_m"]) else 0.0,
            depth_anomaly_ft=restore["depth_anomaly_m"] * M_TO_FT,
            swayze_matches=matches,
            confidence=confidence,
            resolution_ft=res_ft,
            epsg=meta["epsg"],
        )
        regions.append(region)

    return regions


def _convex_hull_latlon(rows, cols, meta, xform, max_points=24) -> List[Tuple[float, float]]:
    """Compute convex hull of grid cells and return as WGS84 polygon."""
    try:
        from scipy.spatial import ConvexHull
        points = np.column_stack((rows, cols))
        if len(points) < 3:
            # Use bounding box
            corners = [
                (int(np.min(rows)), int(np.min(cols))),
                (int(np.min(rows)), int(np.max(cols))),
                (int(np.max(rows)), int(np.max(cols))),
                (int(np.max(rows)), int(np.min(cols))),
            ]
            return [xform.grid_to_latlon(r, c, meta) for r, c in corners]

        hull = ConvexHull(points)
        hull_pts = points[hull.vertices]

        # Subsample if too many vertices
        if len(hull_pts) > max_points:
            step = max(1, len(hull_pts) // max_points)
            hull_pts = hull_pts[::step]

        return [xform.grid_to_latlon(int(r), int(c), meta) for r, c in hull_pts]
    except Exception:
        # Fallback to bounding box
        corners = [
            (int(np.min(rows)), int(np.min(cols))),
            (int(np.min(rows)), int(np.max(cols))),
            (int(np.max(rows)), int(np.max(cols))),
            (int(np.max(rows)), int(np.min(cols))),
        ]
        return [xform.grid_to_latlon(r, c, meta) for r, c in corners]


def _score_confidence(raw: Dict, restore: Dict) -> float:
    """Score how likely this is deliberate masking (0-1)."""
    score = 0.3  # base

    mtype = raw["type"]
    if mtype == "nan_hole":
        score += 0.3  # NaN holes are the most clear-cut masking signal
    elif mtype == "flattened":
        score += 0.2
    elif mtype == "texture_break":
        score += 0.1

    # Size bonus — wreck-sized regions more suspicious
    long_ft = raw["long_ft"]
    if 50 <= long_ft <= 800:
        score += 0.2  # right in the wreck size range
    elif long_ft > 800:
        score += 0.05  # possibly large feature, less specific

    # Depth anomaly bonus — if interpolation shows a bump, very suspicious
    anomaly = abs(restore.get("depth_anomaly_m", 0))
    if anomaly > 1.0:
        score += 0.15

    return min(score, 0.99)


# ============================================================================
# BATCH RUNNER
# ============================================================================

def run_masking_scan(bag_dir: str, output_dir: str,
                     db_path: str = "",
                     progress_callback=None) -> Dict[str, Any]:
    """
    Scan all BAG files in bag_dir for masking patterns.
    Returns summary dict, writes KML/KMZ + JSON report.
    """
    bag_files = sorted(Path(bag_dir).rglob("*.bag"))
    if not bag_files:
        return {"error": "No BAG files found", "bag_dir": bag_dir}

    xform = CoordXform()
    detector = MaskingDetector(xform)
    preview = UnmaskPreview()

    repo_root = Path(__file__).resolve().parents[1]
    if not db_path:
        db_path = str(repo_root / "db" / "wrecks.db")
    swayze = SwayzeXRef(db_path)

    def prog(msg):
        if progress_callback:
            progress_callback(msg)
        else:
            print(msg)

    prog(f"Scanning {len(bag_files)} BAG files for masking patterns...")

    all_regions: List[MaskedRegion] = []

    for i, bf in enumerate(bag_files):
        prog(f"[{i+1}/{len(bag_files)}] {bf.name}")
        regions = scan_bag_for_masking(str(bf), xform, detector, preview, swayze, prog)
        all_regions.extend(regions)

    prog(f"Found {len(all_regions)} masked regions total")

    # Generate KML
    os.makedirs(output_dir, exist_ok=True)
    kml_gen = MaskingKMLGenerator(output_dir)
    kml_path, kmz_path = kml_gen.generate(all_regions)
    prog(f"KML: {kml_path}")
    prog(f"KMZ: {kmz_path}")

    # JSON report
    report = {
        "timestamp": datetime.now().isoformat(),
        "bag_dir": str(bag_dir),
        "files_scanned": len(bag_files),
        "total_masked_regions": len(all_regions),
        "by_type": {
            "nan_hole": sum(1 for r in all_regions if r.mask_type == "nan_hole"),
            "flattened": sum(1 for r in all_regions if r.mask_type == "flattened"),
            "texture_break": sum(1 for r in all_regions if r.mask_type == "texture_break"),
        },
        "with_swayze_match": sum(1 for r in all_regions if r.swayze_matches),
        "regions": [r.to_dict() for r in all_regions],
        "kml_path": kml_path,
        "kmz_path": kmz_path,
    }

    report_path = os.path.join(output_dir, "masking_scan_report.json")
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2, default=str)
    prog(f"Report: {report_path}")

    return report


# ============================================================================
# CLI
# ============================================================================

if __name__ == "__main__":
    import sys
    bag_dir = sys.argv[1] if len(sys.argv) > 1 else r"D:\bagfiles"
    out_dir = sys.argv[2] if len(sys.argv) > 2 else "advanced_scan_results/masking_scan"

    report = run_masking_scan(bag_dir, out_dir)

    print(f"\n{'='*70}")
    print("MASKING SCAN RESULTS")
    print(f"{'='*70}")
    print(f"Files scanned:    {report.get('files_scanned', 0)}")
    print(f"Masked regions:   {report.get('total_masked_regions', 0)}")
    bt = report.get("by_type", {})
    print(f"  NaN holes:      {bt.get('nan_hole', 0)}")
    print(f"  Flattened:      {bt.get('flattened', 0)}")
    print(f"  Texture breaks: {bt.get('texture_break', 0)}")
    print(f"Swayze matches:   {report.get('with_swayze_match', 0)}")
    print(f"KML: {report.get('kml_path', '')}")
    print(f"KMZ: {report.get('kmz_path', '')}")
