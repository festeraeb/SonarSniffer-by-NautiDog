#!/usr/bin/env python3
"""Phase A accel scan: TPU infer + jitter-rs over PNG/JPEG or GeoTIFF inputs.

GeoTIFFs are read via GDAL: center chip downsampled to PNG for /infer; band names
in a scene folder become thermal_timeseries; center coordinates → WGS84 for /jitter.

Example (Holloway PNGs):
  python3 run_accel_scan_files.py --out var/role_bench/out \\
    /path/to/Holloway/

Example (Sentinel-2 scene dir — one scan per scene, red band chip):
  python3 run_accel_scan_files.py --out var/role_bench/s2 \\
    --geotiff-mode scene \\
    /codebase/projects/pipelines/.../sentinel2_aws/
"""
from __future__ import annotations

import argparse
import base64
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import requests

DEFAULT_THERMAL = ["pass-1", "pass-2", "pass-3", "pass-4"]
BAND_RE = re.compile(r"^(.+)\.(blue|green|red|nir\d*|swir\d*|scl)\.tiff?$", re.I)
SCENE_BANDS = ("blue", "green", "red", "nir", "nir08", "swir16", "swir22", "scl")


def png_b64(path: Path) -> str:
    return base64.standard_b64encode(path.read_bytes()).decode("ascii")


def bytes_b64(data: bytes) -> str:
    return base64.standard_b64encode(data).decode("ascii")


def gdalinfo_json(path: Path) -> dict:
    out = subprocess.check_output(
        ["gdalinfo", "-json", str(path)], stderr=subprocess.DEVNULL, text=True
    )
    return json.loads(out)


def geotiff_projected_epsg(path: Path) -> int:
    """Pick projected CRS code from gdalinfo WKT (avoid matching EPSG:4326 in BASEGEOGCRS)."""
    gi = gdalinfo_json(path)
    wkt = (gi.get("coordinateSystem") or {}).get("wkt", "") or ""
    epsgs = [int(x) for x in re.findall(r'ID\["EPSG",(\d+)\]', wkt)]
    projected = [
        e
        for e in epsgs
        if e != 4326 and (32601 <= e <= 32660 or 32701 <= e <= 32760 or e in (3857, 6933))
    ]
    if projected:
        return projected[-1]
    # gdalinfo -proj4 fallback
    try:
        p4 = subprocess.check_output(
            ["gdalinfo", "-proj4", str(path)], stderr=subprocess.DEVNULL, text=True
        )
        m = re.search(r"\+init=epsg:(\d+)", p4, re.I)
        if m:
            return int(m.group(1))
        m = re.search(r"\+proj=utm \+zone=(\d+)", p4)
        if m:
            zone = int(m.group(1))
            return 32600 + zone  # northern hemisphere fleet default
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass
    return 32616


def geotiff_center_wgs84(path: Path) -> tuple[float, float]:
    gi = gdalinfo_json(path)
    cx, cy = gi["cornerCoordinates"]["center"]
    epsg = geotiff_projected_epsg(path)
    line = subprocess.check_output(
        [
            "gdaltransform",
            "-s_srs",
            f"EPSG:{epsg}",
            "-t_srs",
            "EPSG:4326",
        ],
        input=f"{cx} {cy}\n",
        text=True,
    ).strip()
    lon, lat = map(float, line.split()[:2])
    return lat, lon


def geotiff_chip_png(path: Path, chip_px: int = 2048, out_px: int = 512) -> bytes:
    gi = gdalinfo_json(path)
    w, h = int(gi["size"][0]), int(gi["size"][1])
    chip = min(chip_px, w, h)
    xoff = max(0, (w - chip) // 2)
    yoff = max(0, (h - chip) // 2)
    with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as tmp:
        tmp_path = Path(tmp.name)
    try:
        subprocess.run(
            [
                "gdal_translate",
                "-q",
                "-of",
                "PNG",
                "-srcwin",
                str(xoff),
                str(yoff),
                str(chip),
                str(chip),
                "-outsize",
                str(out_px),
                str(out_px),
                str(path),
                str(tmp_path),
            ],
            check=True,
        )
        return tmp_path.read_bytes()
    finally:
        tmp_path.unlink(missing_ok=True)


def scene_groups(tifs: list[Path]) -> dict[str, list[Path]]:
    groups: dict[str, list[Path]] = {}
    for p in tifs:
        m = BAND_RE.match(p.name)
        key = m.group(1) if m else p.stem
        groups.setdefault(key, []).append(p)
    for k in groups:
        groups[k].sort(key=lambda x: x.name)
    return groups


def pick_glint_band(bands: list[Path]) -> Path:
    for suffix in ("red", "nir", "green", "blue"):
        for p in bands:
            if p.name.lower().endswith(f".{suffix}.tif") or p.name.lower().endswith(
                f".{suffix}.tiff"
            ):
                return p
    return bands[0]


def band_labels(bands: list[Path]) -> list[str]:
    labels = []
    for p in bands:
        m = BAND_RE.match(p.name)
        labels.append(m.group(2) if m else p.stem)
    return labels


def scan_one_raster(
    path: Path,
    *,
    image_b64: str,
    tile_id: str,
    tpu: str,
    jitter: str,
    lat: float,
    lon: float,
    depth_m: float,
    thermal: list[str],
    source_file: str | None = None,
) -> dict:
    meta = {
        "tile_id": tile_id,
        "lat": lat,
        "lon": lon,
        "pass_id": path.stem,
        "source_file": source_file or str(path.resolve()),
    }
    tpu_r = requests.post(
        f"{tpu.rstrip('/')}/infer",
        json={"image_base64": image_b64, "meta": meta},
        timeout=300,
    )
    tpu_r.raise_for_status()
    tpu_j = tpu_r.json()

    jit_r = requests.post(
        f"{jitter.rstrip('/')}/jitter",
        json={
            "tile_id": tile_id,
            "thermal_timeseries": thermal,
            "coordinates": {"lat": lat, "lon": lon},
            "depth_estimate_m": depth_m,
        },
        timeout=60,
    )
    jit_r.raise_for_status()
    jit_j = jit_r.json()

    return {
        "source_file": source_file or str(path),
        "glint_band": str(path),
        "tile_id": tile_id,
        "coordinates": {"lat": lat, "lon": lon},
        "thermal_timeseries": thermal,
        "tpu_scan": {
            "detection_count": len(tpu_j.get("detections") or []),
            "top_detections": (tpu_j.get("detections") or [])[:8],
            "used_tpu": tpu_j.get("used_tpu"),
            "took_s": tpu_j.get("took_s"),
        },
        "jitter_signature": jit_j,
    }


def scan_one_png(
    path: Path,
    *,
    tpu: str,
    jitter: str,
    lat: float,
    lon: float,
    depth_m: float,
    thermal: list[str],
) -> dict:
    tile_id = f"{path.stem}@{path.parent.name}"
    return scan_one_raster(
        path,
        image_b64=png_b64(path),
        tile_id=tile_id,
        tpu=tpu,
        jitter=jitter,
        lat=lat,
        lon=lon,
        depth_m=depth_m,
        thermal=thermal,
    )


def collect_paths(paths: list[str], *, geotiff: bool) -> tuple[list[Path], list[Path]]:
    pngs: list[Path] = []
    tifs: list[Path] = []
    for raw in paths:
        path = Path(raw)
        if path.is_dir():
            if geotiff:
                tifs.extend(sorted(path.glob("*.tif")))
                tifs.extend(sorted(path.glob("*.tiff")))
            pngs.extend(sorted(path.glob("*.png")))
            pngs.extend(sorted(path.glob("*.jpg")))
            pngs.extend(sorted(path.glob("*.jpeg")))
        elif path.is_file():
            suf = path.suffix.lower()
            if suf in (".tif", ".tiff"):
                tifs.append(path)
            elif suf in (".png", ".jpg", ".jpeg"):
                pngs.append(path)
            else:
                print(f"skip unknown type: {path}", file=sys.stderr)
        else:
            print(f"skip missing: {path}", file=sys.stderr)
    return pngs, tifs


def main() -> int:
    ap = argparse.ArgumentParser(description="Accel scan over PNG/JPEG/GeoTIFF")
    ap.add_argument("paths", nargs="+", help="files or directories")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--tpu", default="http://10.0.0.201:8092")
    ap.add_argument("--jitter", default="http://10.0.0.61:8180")
    ap.add_argument("--lat", type=float, default=None, help="WGS84 (PNG default 42.331)")
    ap.add_argument("--lon", type=float, default=None, help="WGS84 (PNG default -83.048)")
    ap.add_argument("--depth-m", type=float, default=42.0)
    ap.add_argument("--thermal", nargs="*", default=None)
    ap.add_argument(
        "--geotiff-mode",
        choices=("scene", "band"),
        default="scene",
        help="scene: one scan per S2 scene; band: each .tif separately",
    )
    ap.add_argument("--chip-px", type=int, default=2048, help="GeoTIFF srcwin size")
    ap.add_argument("--out-px", type=int, default=512, help="chip PNG edge for TPU")
    ap.add_argument("--limit-scenes", type=int, default=0, help="0 = all scenes")
    args = ap.parse_args()

    pngs, tifs = collect_paths(args.paths, geotiff=True)
    if not pngs and not tifs:
        print("no PNG/JPEG/GeoTIFF files found", file=sys.stderr)
        return 1

    default_lat = 42.331 if args.lat is None else args.lat
    default_lon = -83.048 if args.lon is None else args.lon
    default_thermal = list(args.thermal) if args.thermal else list(DEFAULT_THERMAL)

    args.out.mkdir(parents=True, exist_ok=True)
    results: list[dict] = []

    for f in pngs:
        lat = args.lat if args.lat is not None else default_lat
        lon = args.lon if args.lon is not None else default_lon
        print(f"scan PNG {f} ...", flush=True)
        row = scan_one_png(
            f,
            tpu=args.tpu,
            jitter=args.jitter,
            lat=lat,
            lon=lon,
            depth_m=args.depth_m,
            thermal=default_thermal,
        )
        results.append(row)
        safe = re.sub(r"[^\w.-]+", "_", f.stem)
        (args.out / f"{safe}_jitter.json").write_text(
            json.dumps(row["jitter_signature"], indent=2)
        )

    if tifs:
        if args.geotiff_mode == "band":
            work: list[tuple[str, list[Path]]] = [(p.stem, [p]) for p in tifs]
        else:
            work = list(scene_groups(tifs).items())
            if args.limit_scenes > 0:
                work = work[: args.limit_scenes]

        for scene_id, bands in work:
            glint = pick_glint_band(bands)
            thermal = band_labels(bands) if args.thermal is None else list(args.thermal)
            lat, lon = geotiff_center_wgs84(glint)
            print(
                f"scan GeoTIFF scene={scene_id} glint={glint.name} "
                f"({len(bands)} bands, {lat:.4f},{lon:.4f}) ...",
                flush=True,
            )
            chip = geotiff_chip_png(glint, chip_px=args.chip_px, out_px=args.out_px)
            row = scan_one_raster(
                glint,
                image_b64=bytes_b64(chip),
                tile_id=scene_id,
                tpu=args.tpu,
                jitter=args.jitter,
                lat=lat,
                lon=lon,
                depth_m=args.depth_m,
                thermal=thermal,
                source_file=str(glint),
            )
            row["scene_id"] = scene_id
            row["bands"] = [p.name for p in bands]
            results.append(row)
            safe = re.sub(r"[^\w.-]+", "_", scene_id)
            (args.out / f"{safe}_jitter.json").write_text(
                json.dumps(row["jitter_signature"], indent=2)
            )

    packet = {
        "mission": "sunken_ship_anchor_hunt",
        "files_scanned": len(results),
        "scans": results,
    }
    if results:
        packet["coordinates"] = results[0]["coordinates"]
    (args.out / "accel_scan_packet.json").write_text(json.dumps(packet, indent=2))
    print(json.dumps(packet, indent=2)[:2500])
    print(f"\nwrote {args.out}/accel_scan_packet.json ({len(results)} items)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
