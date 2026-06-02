#!/usr/bin/env python3
"""
Deep examination of CESAROPS remote-sensing assets (not a 3-line skim).

Produces rs_profile dict: sensor hints, CRS, dimensions, sidecars, STAC fields,
filename tokens, gdalinfo (when available), and examine_blob for embedding.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path
from typing import Any

RS_EXTENSIONS = {
    ".tif", ".tiff", ".gtiff", ".vrt", ".jp2", ".j2k",
    ".nc", ".nc4", ".hdf", ".hdf5", ".h5",
    ".bag", ".mbag",
    ".shp", ".shx", ".dbf", ".prj", ".cpg",
    ".geojson", ".kml", ".kmz", ".gpx", ".gpkg",
    ".aux.xml", ".tfw", ".wld", ".hdr", ".img",
    ".las", ".laz",
}
RS_PATH_RE = re.compile(
    r"(geotiff|sentinel|landsat|hls|viirs|modis|sar|rtc|coh|insar|"
    r"census_raw|spm|ndvi|glint|ripple|tile|wreckhunter|sonar|bag|"
    r"shapefile|geojson|stac|earthdata|copernicus|asf|alos|palsar)",
    re.I,
)
SENSOR_FROM_NAME = [
    (re.compile(r"S2A_|S2B_|S2_|sentinel.?2|s2_l2a", re.I), "Sentinel-2"),
    (re.compile(r"L30\.|L50\.|LC08|LC09|landsat|HLS\.L", re.I), "Landsat/HLS"),
    (re.compile(r"S30\.|HLS\.S", re.I), "Sentinel-2 HLS"),
    (re.compile(r"VIIRS|VNP", re.I), "VIIRS"),
    (re.compile(r"MODIS|MOD\d", re.I), "MODIS"),
    (re.compile(r"SAR|RTC|GRD|SLC|sentinel-1", re.I), "SAR"),
    (re.compile(r"bag|ros", re.I), "ROS bag"),
    (re.compile(r"\.las|lidar", re.I), "LiDAR"),
]
HLS_NAME = re.compile(
    r"(?P<prefix>HLS)\.(?P<sat>L30|S30)\.(?P<tile>T\w+)\.(?P<acq>[\dT]+)\.(?P<ver>v[\d.]+)\.(?P<band>B\d+)",
    re.I,
)


def is_remote_sensing_path(path: Path) -> bool:
    ext = path.suffix.lower()
    if ext in RS_EXTENSIONS:
        return True
    if ext == ".xml" and path.name.endswith(".aux.xml"):
        return True
    return bool(RS_PATH_RE.search(str(path)))


def sensor_from_path(path: Path) -> str:
    name = path.name
    for pat, label in SENSOR_FROM_NAME:
        if pat.search(name) or pat.search(str(path.parent)):
            return label
    return "unknown"


def read_bytes(path: Path, limit: int) -> bytes:
    try:
        with path.open("rb") as f:
            return f.read(limit)
    except OSError:
        return b""


def read_text(path: Path, limit: int) -> str:
    try:
        with path.open("r", encoding="utf-8", errors="ignore") as f:
            return f.read(limit)
    except OSError:
        return ""


def sidecar_paths(raster: Path) -> list[Path]:
    base = raster.with_suffix("")
    names = [
        raster.with_name(raster.name + ".aux.xml"),
        base.with_suffix(".aux.xml"),
        raster.with_suffix(".tfw"),
        base.with_suffix(".tfw"),
        raster.with_suffix(".prj"),
        base.with_suffix(".prj"),
        raster.with_suffix(".xml"),
    ]
    out: list[Path] = []
    seen: set[str] = set()
    for p in names:
        s = str(p)
        if s not in seen and p.is_file():
            seen.add(s)
            out.append(p)
    return out


def gdalinfo_json(path: Path, timeout: int = 120) -> dict[str, Any] | None:
    try:
        proc = subprocess.run(
            ["gdalinfo", "-json", "-stats", str(path)],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        if proc.returncode != 0:
            proc = subprocess.run(
                ["gdalinfo", "-json", str(path)],
                capture_output=True,
                text=True,
                timeout=timeout,
            )
        if proc.returncode == 0 and proc.stdout.strip():
            return json.loads(proc.stdout)
    except (subprocess.SubprocessError, json.JSONDecodeError, FileNotFoundError):
        pass
    return None


def parse_hls_filename(name: str) -> dict[str, str]:
    m = HLS_NAME.search(name)
    if not m:
        return {}
    return {k: m.group(k) for k in m.groupdict() if m.group(k)}


def tiff_ascii_tags(blob: bytes) -> dict[str, str]:
    """Extract readable GeoTIFF/STAC-ish ASCII from header window."""
    found: dict[str, str] = {}
    for key in (
        b"GeoAsciiParamsTag",
        b"GDAL_METADATA",
        b"TIFFTAG_IMAGEDESCRIPTION",
        b"MODELTIEPOINTTAG",
        b"RPC",
        b"Sentinel",
        b"Landsat",
        b"EPSG",
        b"PROJCS",
        b"GEOGCS",
    ):
        idx = blob.find(key)
        if idx >= 0:
            chunk = blob[idx : idx + 400].decode("latin-1", errors="ignore")
            found[key.decode("ascii", errors="ignore")] = chunk[:300]
    return found


def profile_geotiff(path: Path, cfg: dict[str, Any]) -> dict[str, Any]:
    prof: dict[str, Any] = {
        "kind": "geotiff_raster",
        "sensor_guess": sensor_from_path(path),
        "hls_parse": parse_hls_filename(path.name),
        "size_bytes": path.stat().st_size,
    }
    hlim = int(cfg.get("max_rs_raster_header_bytes", 524288))
    prof["header_ascii"] = tiff_ascii_tags(read_bytes(path, hlim))

    gi = gdalinfo_json(path)
    if gi:
        prof["gdalinfo"] = {
            "driver": gi.get("driverShortName") or gi.get("driverName"),
            "size": gi.get("size"),
            "geoTransform": gi.get("geoTransform"),
            "coordinateSystem": (gi.get("coordinateSystem") or {}).get("wkt", "")[:2000],
            "bands": [
                {
                    "band": b.get("band"),
                    "type": b.get("type"),
                    "colorInterpretation": b.get("colorInterpretation"),
                    "min": (b.get("computedMin") or b.get("minimum")),
                    "max": (b.get("computedMax") or b.get("maximum")),
                }
                for b in (gi.get("bands") or [])[:16]
            ],
            "metadata": gi.get("metadata"),
        }
        desc = ""
        for band in gi.get("bands") or []:
            for item in band.get("metadata", {}).get("", []) or []:
                if isinstance(item, dict) and item.get("name") == "STATISTICS_VALID_PERCENT":
                    prof.setdefault("band_stats", []).append(item)
        if gi.get("stac"):
            prof["stac"] = gi["stac"]

    slimit = int(cfg.get("max_sidecar_bytes", 262144))
    sidecars: dict[str, str] = {}
    for sc in sidecar_paths(path):
        sidecars[sc.name] = read_text(sc, slimit)
    if sidecars:
        prof["sidecars"] = sidecars

    return prof


def profile_netcdf(path: Path, cfg: dict[str, Any]) -> dict[str, Any]:
    prof: dict[str, Any] = {
        "kind": "netcdf",
        "sensor_guess": sensor_from_path(path),
        "size_bytes": path.stat().st_size,
    }
    try:
        import netCDF4  # type: ignore

        with netCDF4.Dataset(str(path), "r") as ds:
            prof["dimensions"] = {k: len(v) for k, v in ds.dimensions.items()}
            prof["variables"] = list(ds.variables.keys())[:64]
            prof["global_attrs"] = {
                k: str(ds.getncattr(k))[:500]
                for k in ds.ncattrs()
            }
    except Exception as e:
        prof["netcdf4_error"] = str(e)
        blob = read_bytes(path, int(cfg.get("max_rs_raster_header_bytes", 524288)))
        prof["header_ascii"] = tiff_ascii_tags(blob)  # reuse ascii scan
        prof["header_preview"] = blob[:8000].decode("latin-1", errors="ignore")

    return prof


def profile_geojson(path: Path, cfg: dict[str, Any]) -> dict[str, Any]:
    limit = int(cfg.get("max_text_examine_bytes", 65536))
    text = read_text(path, limit)
    prof: dict[str, Any] = {"kind": "geojson", "size_bytes": path.stat().st_size}
    try:
        data = json.loads(text)
        prof["type"] = data.get("type")
        if data.get("type") == "FeatureCollection":
            prof["feature_count"] = len(data.get("features") or [])
        if "stac_version" in data or data.get("type") == "Feature":
            prof["stac_like"] = True
            prof["stac_fields"] = {
                k: data.get(k)
                for k in ("id", "collection", "datetime", "properties", "assets")
                if k in data
            }
    except json.JSONDecodeError as e:
        prof["json_error"] = str(e)
        prof["text_preview"] = text[:4000]
    return prof


def profile_vector(path: Path, cfg: dict[str, Any]) -> dict[str, Any]:
    prof: dict[str, Any] = {
        "kind": "vector",
        "suffix": path.suffix.lower(),
        "sensor_guess": sensor_from_path(path),
        "size_bytes": path.stat().st_size,
    }
    if path.suffix.lower() == ".prj":
        prof["prj_wkt"] = read_text(path, int(cfg.get("max_sidecar_bytes", 262144)))
    return prof


def profile_generic_rs(path: Path, cfg: dict[str, Any]) -> dict[str, Any]:
    ext = path.suffix.lower()
    if ext in {".tif", ".tiff", ".gtiff", ".vrt", ".jp2"}:
        return profile_geotiff(path, cfg)
    if ext in {".nc", ".nc4", ".hdf", ".hdf5", ".h5"}:
        return profile_netcdf(path, cfg)
    if ext == ".geojson" or (ext == ".json" and RS_PATH_RE.search(str(path))):
        return profile_geojson(path, cfg)
    if ext in {".shp", ".shx", ".dbf", ".prj", ".cpg"}:
        return profile_vector(path, cfg)
    if path.name.endswith(".aux.xml"):
        return {
            "kind": "geotiff_aux_xml",
            "content": read_text(path, int(cfg.get("max_sidecar_bytes", 262144))),
        }
    limit = int(cfg.get("max_text_examine_bytes", 65536))
    return {
        "kind": "rs_related_text",
        "content": read_text(path, limit),
        "sensor_guess": sensor_from_path(path),
    }


def build_examine_blob(path: Path, prof: dict[str, Any]) -> str:
    """Rich text bundle for vector embedding / human review."""
    parts = [
        f"PATH: {path}",
        f"KIND: {prof.get('kind')}",
        f"SENSOR_GUESS: {prof.get('sensor_guess', '')}",
    ]
    if prof.get("hls_parse"):
        parts.append(f"HLS: {json.dumps(prof['hls_parse'])}")
    if prof.get("gdalinfo"):
        parts.append(f"GDAL: {json.dumps(prof['gdalinfo'], default=str)[:12000]}")
    if prof.get("global_attrs"):
        parts.append(f"NC_ATTRS: {json.dumps(prof['global_attrs'], default=str)[:8000]}")
    if prof.get("variables"):
        parts.append(f"NC_VARS: {prof['variables']}")
    if prof.get("sidecars"):
        for name, body in prof["sidecars"].items():
            parts.append(f"SIDEcar {name}:\n{body[:8000]}")
    if prof.get("header_ascii"):
        parts.append(f"HEADER_ASCII: {json.dumps(prof['header_ascii'])[:6000]}")
    if prof.get("stac_fields"):
        parts.append(f"STAC: {json.dumps(prof['stac_fields'], default=str)[:8000]}")
    if prof.get("prj_wkt"):
        parts.append(f"PRJ: {prof['prj_wkt'][:4000]}")
    if prof.get("content"):
        parts.append(f"CONTENT:\n{prof['content'][:16000]}")
    return "\n\n".join(parts)[:48000]


def shallow_profile(path: Path) -> dict[str, Any]:
    """Fast RS tag only — queue for parallel deep scan on fleet workers."""
    try:
        size = path.stat().st_size
    except OSError:
        size = 0
    return {
        "kind": "remote_sensing_pending",
        "sensor_guess": sensor_from_path(path),
        "hls_parse": parse_hls_filename(path.name),
        "size_bytes": size,
        "ext": path.suffix.lower(),
    }


def deep_profile(path: Path, rs_cfg: dict[str, Any]) -> tuple[dict[str, Any], str]:
    prof = profile_generic_rs(path, rs_cfg)
    blob = build_examine_blob(path, prof)
    return prof, blob


def apply_deep_to_row(row: dict[str, Any], path: Path, rs_cfg: dict[str, Any]) -> dict[str, Any]:
    """Run deep profile and merge into catalog row."""
    prof, blob = deep_profile(path, rs_cfg)
    row = dict(row)
    row["rs_profile"] = prof
    row["examine_blob"] = blob
    row["sensor_guess"] = prof.get("sensor_guess") or sensor_from_path(path)
    row["examine_depth"] = "deep"
    row["deep_scan_status"] = "complete"
    row["deep_scan_worker"] = row.get("deep_scan_worker") or os.environ.get("RS_DEEP_WORKER_ID", "local")
    return row
