#!/usr/bin/env python3
"""
Run SonarSniffer pipeline on an RSD, then send mosaic/waterfall PNGs to P106 vision scout.

Usage:
  python3 scripts/analyze_sonarsniffer_outputs.py path/to/file.RSD
  SCOUT_URL=http://127.0.0.1:5570 python3 scripts/analyze_sonarsniffer_outputs.py ...

Requires: scout on :5570 (start_vision_workers.sh gpu), parse_cli built with GStreamer.
"""
from __future__ import annotations

import argparse
import base64
import json
import os
import subprocess
import sys
from pathlib import Path

import urllib.request

REPO = Path(__file__).resolve().parents[1]
PARSE_CLI = REPO / "sonarsniffer" / "target" / "release" / "parse_cli"
SCOUT_URL = os.environ.get("SCOUT_URL", "http://127.0.0.1:5570")


def scout_health() -> bool:
    try:
        with urllib.request.urlopen(f"{SCOUT_URL}/health", timeout=3) as r:
            return r.status == 200
    except Exception:
        return False


def analyze_image(path: Path, tile_id: str) -> dict:
    raw = path.read_bytes()
    b64 = base64.b64encode(raw).decode("ascii")
    body = json.dumps(
        {
            "tile_id": tile_id,
            "image_b64": b64,
            "task": "anomaly_detection",
            "lat": 0.0,
            "lon": 0.0,
        }
    ).encode()
    req = urllib.request.Request(
        f"{SCOUT_URL}/analyze",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read().decode())


def find_review_images(out_dir: Path) -> list[Path]:
    patterns = ("*mosaic*.png", "*waterfall*.png", "*Mosaic*.png", "*Waterfall*.png")
    found: list[Path] = []
    for pat in patterns:
        found.extend(out_dir.rglob(pat))
    # de-dupe, prefer smaller filenames for quick pass
    seen = set()
    unique = []
    for p in sorted(found, key=lambda x: x.stat().st_size if x.exists() else 0):
        if p not in seen and p.is_file():
            seen.add(p)
            unique.append(p)
    return unique[:12]


def run_parse(rsd: Path, out_parent: Path | None, light: bool) -> Path:
    if not PARSE_CLI.exists():
        raise SystemExit(f"Missing {PARSE_CLI} — run: cd sonarsniffer && cargo build --release --bin parse_cli")

    cmd = [str(PARSE_CLI), str(rsd)]
    if light:
        cmd.append("--light")
    else:
        cmd.extend(["--no-arcgis", "--no-viewer", "--no-mbtiles", "--no-kmz"])
    if out_parent:
        cmd.extend(["--output-dir", str(out_parent)])

    print("[pipeline]", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True, cwd=REPO)

    # parse_cli writes next to file or --output-dir
    if out_parent:
        return out_parent
    return rsd.parent / "output"


def main() -> None:
    ap = argparse.ArgumentParser(description="SonarSniffer → vision scout review")
    ap.add_argument("rsd", type=Path, help="Garmin .RSD (or supported sonar file)")
    ap.add_argument("--light", action="store_true", help="Skip video/mosaic (faster)")
    ap.add_argument("--output-dir", type=Path, default=None)
    ap.add_argument("--skip-parse", action="store_true", help="Only vision-scan existing output dir")
    args = ap.parse_args()

    rsd = args.rsd.resolve()
    if not args.skip_parse:
        if not rsd.exists():
            raise SystemExit(f"Not found: {rsd}")
        out_dir = run_parse(rsd, args.output_dir, args.light)
    else:
        out_dir = (args.output_dir or rsd.parent / "output").resolve()
        if not out_dir.is_dir():
            raise SystemExit(f"Output dir missing: {out_dir}")

    print(f"[vision] output dir: {out_dir}", flush=True)
    if not scout_health():
        raise SystemExit(
            f"Scout not reachable at {SCOUT_URL}. Start:\n"
            "  VISION_MODE=gpu bash scripts/start_vision_workers.sh start"
        )

    images = find_review_images(out_dir)
    if not images:
        print("[vision] no mosaic/waterfall PNGs found — try full pipeline without --light", flush=True)
        sys.exit(0)

    report = {"output_dir": str(out_dir), "rsd": str(rsd), "findings": []}
    for img in images:
        tid = img.stem[:64]
        print(f"[vision] analyze {img.name} ...", flush=True)
        try:
            finding = analyze_image(img, tid)
            finding["file"] = str(img)
            report["findings"].append(finding)
            flag = "ANOMALY" if finding.get("has_anomaly") else "ok"
            print(
                f"  {flag} conf={finding.get('confidence', 0):.2f} "
                f"type={finding.get('anomaly_type')} — {finding.get('description', '')[:80]}",
                flush=True,
            )
        except Exception as e:
            print(f"  ERROR {img.name}: {e}", flush=True)
            report["findings"].append({"file": str(img), "error": str(e)})

    report_path = out_dir / "vision_scout_report.json"
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[vision] wrote {report_path}", flush=True)


if __name__ == "__main__":
    main()
