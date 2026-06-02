#!/usr/bin/env python3
"""Resolve and optionally execute a unified mission spec for n8n worker bees.

This keeps one pipeline contract and changes only variables by profile.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from copy import deepcopy
from pathlib import Path
from typing import Any, Dict


REPO = Path(__file__).resolve().parents[1]
PROFILES_PATH = REPO / "config" / "unified_pipeline_profiles.json"
DEFAULT_TEMPLATE_PATH = REPO / "missions" / "dual_use_mission_template.json"
GENERATED_DIR = REPO / "missions" / "generated"


def deep_merge(dst: Dict[str, Any], src: Dict[str, Any]) -> Dict[str, Any]:
    for k, v in src.items():
        if k in dst and isinstance(dst[k], dict) and isinstance(v, dict):
            deep_merge(dst[k], v)
        else:
            dst[k] = v
    return dst


def load_json(path: Path) -> Dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def parse_payload(args: argparse.Namespace) -> Dict[str, Any]:
    if args.payload_file:
        return load_json(Path(args.payload_file))
    if args.payload_json:
        return json.loads(args.payload_json)
    payload: Dict[str, Any] = {}
    if args.pipeline_type:
        payload["pipeline_type"] = args.pipeline_type
    if args.mission_id:
        payload["mission_id"] = args.mission_id
    if args.bbox:
        payload["bbox"] = [float(x.strip()) for x in args.bbox.split(",")]
    if args.date_start and args.date_end:
        payload["date_range"] = [args.date_start, args.date_end]
    if args.overrides_json:
        payload["overrides"] = json.loads(args.overrides_json)
    return payload


def resolve_spec(payload: Dict[str, Any]) -> Dict[str, Any]:
    profiles_doc = load_json(PROFILES_PATH)
    profiles = profiles_doc.get("profiles", {})
    pipeline_type = str(payload.get("pipeline_type") or "water_wreck_hunt")
    if pipeline_type not in profiles:
        available = ", ".join(sorted(profiles.keys()))
        raise ValueError(f"Unknown pipeline_type: {pipeline_type}. Available: {available}")

    template_rel = profiles_doc.get("default_template", "missions/dual_use_mission_template.json")
    template_path = REPO / template_rel
    if not template_path.exists():
        template_path = DEFAULT_TEMPLATE_PATH

    spec = load_json(template_path)
    profile_patch = profiles[pipeline_type].get("patch", {})
    deep_merge(spec, deepcopy(profile_patch))

    # Allow payload top-level mission fields.
    passthrough_fields = [
        "mission_id",
        "target_name",
        "bbox",
        "date_range",
        "weather_filter",
        "sensors",
        "domain",
        "objective",
    ]
    for field in passthrough_fields:
        if field in payload:
            spec[field] = payload[field]

    # Optional deep overrides for any nested section.
    if isinstance(payload.get("overrides"), dict):
        deep_merge(spec, deepcopy(payload["overrides"]))

    # Stamp metadata for traceability.
    spec.setdefault("meta", {})
    spec["meta"]["pipeline_type"] = pipeline_type
    spec["meta"]["resolved_by"] = "unified_pipeline_worker_bee.py"

    return spec


def write_spec(spec: Dict[str, Any]) -> Path:
    GENERATED_DIR.mkdir(parents=True, exist_ok=True)
    mission_id = str(spec.get("mission_id") or "generated_mission")
    out_path = GENERATED_DIR / f"{mission_id}.json"
    out_path.write_text(json.dumps(spec, indent=2), encoding="utf-8")
    return out_path


def run_mission(spec_path: Path) -> int:
    cmd = [sys.executable, str(REPO / "mission_control.py"), "--spec", str(spec_path)]
    proc = subprocess.run(cmd, cwd=str(REPO))
    return int(proc.returncode)


def main() -> int:
    ap = argparse.ArgumentParser(description="Resolve and optionally execute unified mission specs")
    ap.add_argument("--payload-file", help="Path to JSON payload from n8n")
    ap.add_argument("--payload-json", help="Inline JSON payload")
    ap.add_argument("--pipeline-type", help="Profile key in unified_pipeline_profiles.json")
    ap.add_argument("--mission-id")
    ap.add_argument("--bbox", help="lat_min,lon_min,lat_max,lon_max")
    ap.add_argument("--date-start")
    ap.add_argument("--date-end")
    ap.add_argument("--overrides-json", help="Inline deep overrides JSON")
    ap.add_argument("--execute", action="store_true", help="Execute mission_control with resolved spec")
    args = ap.parse_args()

    payload = parse_payload(args)
    spec = resolve_spec(payload)
    out_path = write_spec(spec)

    result: Dict[str, Any] = {
        "ok": True,
        "spec_path": str(out_path),
        "mission_id": spec.get("mission_id"),
        "pipeline_type": spec.get("meta", {}).get("pipeline_type"),
        "execute": bool(args.execute),
    }

    if args.execute:
        rc = run_mission(out_path)
        result["mission_control_exit_code"] = rc
        result["ok"] = rc == 0

    print(json.dumps(result, indent=2))
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    raise SystemExit(main())
