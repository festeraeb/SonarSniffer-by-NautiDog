#!/usr/bin/env python3
"""Orchestrate Lake Michigan processing locally.

Features:
- Scan an inputs directory for GeoTIFFs for Lake Michigan or South Fox->Milwaukee region.
- Detect older/rough files by pixel size and flag them.
- Ensure per-tile `.geo.json` sidecars exist (write them if missing).
- Optionally attempt to download Landsat (8/9) scenes and ICESat wrappers (if requested).
- Run the drift measurement tool and write a JSON report.

This script is intended to be run locally once you attach the hard drive with inputs.
"""
from pathlib import Path
import argparse
import json
import sys
import subprocess
from pprint import pprint

try:
    import rasterio
except Exception:
    print("rasterio is required. Install with: python -m pip install rasterio")
    raise

from rasterio.transform import Affine
import yaml

DEFAULT_INPUTS = "inputs"

# Load named regions from config/regions.yaml if present, else fall back to defaults
REGIONS = None
regions_path = Path("config/regions.yaml")
if regions_path.exists():
    try:
        with open(regions_path, "r") as fh:
            REGIONS = yaml.safe_load(fh)
    except Exception:
        REGIONS = None

if not REGIONS:
    # Fallback defaults
    REGIONS = {
        "lake_michigan": {"bbox": [-92.0, 41.0, -86.0, 46.5], "desc": "Lake Michigan approximate bbox"},
        "south_fox_milwaukee": {"bbox": [-88.2, 42.5, -87.5, 43.25], "desc": "South Fox to Milwaukee area"},
    }


def write_sidecar(ds_path: Path):
    sidecar = ds_path.with_suffix(ds_path.suffix + ".geo.json")
    if sidecar.exists():
        return sidecar
    with rasterio.open(ds_path) as ds:
        data = {
            "transform": list(ds.transform),
            "crs": ds.crs.to_string() if ds.crs else None,
            "width": ds.width,
            "height": ds.height,
        }
    sidecar.write_text(json.dumps(data))
    return sidecar


def detect_rough(ds_path: Path, threshold_meters=30.0):
    with rasterio.open(ds_path) as ds:
        t = ds.transform
        # pixel size: abs(a) for x, abs(e) for y
        px = abs(t.a)
        py = abs(t.e)
        is_rough = max(px, py) >= threshold_meters
        return {"px": px, "py": py, "is_rough": is_rough}


def run_drift_measure(tiles_dir: Path, reference: Path, config_path: Path = None, out: Path = None):
    cmd = [sys.executable, "tools/drift_measure/drift_measure.py", "--mode", "tiles_vs_ref", "--tiles-dir", str(tiles_dir), "--reference", str(reference)]
    if config_path:
        cmd += ["--config", str(config_path)]
    if out:
        cmd += ["--out", str(out)]
    print("Running drift measurement:", " ".join(cmd))
    subprocess.check_call(cmd)


def find_tiffs(inputs_dir: Path, region_bbox=None):
    # Simple: find all .tif files in inputs_dir (no spatial intersection for now).
    return sorted(inputs_dir.rglob("*.tif"))


def main():
    p = argparse.ArgumentParser()
    group = p.add_mutually_exclusive_group(required=True)
    group.add_argument("--region", choices=list(REGIONS.keys()), help="Named region from config/regions.yaml")
    group.add_argument("--bbox", help="Custom bbox as minx,miny,maxx,maxy (lon/lat)")
    p.add_argument("--list-regions", action="store_true", help="List available named regions and exit")
    p.add_argument("--inputs", default=DEFAULT_INPUTS)
    p.add_argument("--reference", help="Reference GeoTIFF path for drift checks", required=True)
    p.add_argument("--download", action="store_true", help="Attempt to download missing Landsat/ICESat data (may require credentials)")
    p.add_argument("--config", default="config/pipeline_config.yaml", help="Pipeline config (YAML)")
    p.add_argument("--out", default="reports/lake_michigan_run.json", help="Report output file")
    args = p.parse_args()

    if args.list_regions:
        print("Available regions:")
        for k, v in REGIONS.items():
            print(f"- {k}: {v.get('desc', '')} bbox={v.get('bbox')}")
        return

    # Determine bbox
    if args.region:
        bbox = REGIONS[args.region]["bbox"]
        region_name = args.region
    else:
        try:
            parts = [float(x) for x in args.bbox.split(",")]
            if len(parts) != 4:
                raise ValueError()
            bbox = parts
            region_name = "custom"
        except Exception:
            print("Invalid --bbox. Use minx,miny,maxx,maxy")
            return

    inputs = Path(args.inputs)
    inputs.mkdir(parents=True, exist_ok=True)
    report = {"region": region_name, "bbox": bbox, "inputs_checked": [], "rough_files": [], "sidecars_written": [], "drift_report": None}

    tiffs = find_tiffs(inputs, bbox) if inputs.exists() else []
    print(f"Found {len(tiffs)} tif(s) in {inputs} for region {region_name}")

    for t in tiffs:
        info = detect_rough(t)
        report["inputs_checked"].append({"path": str(t), "px": info["px"], "py": info["py"], "is_rough": info["is_rough"]})
        if info["is_rough"]:
            report["rough_files"].append(str(t))
        sc = write_sidecar(t)
        report["sidecars_written"].append(str(sc))

    # If download requested, call downloader wrapper
    if args.download:
        try:
            from tools.downloaders.landsat_downloader import download_landsat_for_bbox

            print("Attempting Landsat download (may require credentials)")
            download_landsat_for_bbox(bbox, inputs)
        except Exception as e:
            print("Landsat downloader failed or not installed:", e)

    # Run drift measurement
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        run_drift_measure(inputs, Path(args.reference), Path(args.config), out_path.with_suffix(".drift.json"))
        report["drift_report"] = str(out_path.with_suffix(".drift.json"))
    except subprocess.CalledProcessError as e:
        print("Drift measurement failed:", e)

    with open(out_path, "w") as fh:
        json.dump(report, fh, indent=2)

    print("Report written to", out_path)


if __name__ == "__main__":
    main()
