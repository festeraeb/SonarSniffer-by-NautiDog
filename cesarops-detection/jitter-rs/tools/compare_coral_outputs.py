#!/usr/bin/env python3
"""Offline A/B: sigmoid [1,1] vs logits [1,2] on shared featurize() fixtures."""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

# reuse worker logic
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from coral_jitter_worker import (  # noqa: E402
    agreement_score,
    featurize,
    stub_certainty,
)

FIXTURES = [
    {
        "tile_id": "agree_high",
        "thermal_timeseries": ["b1", "b2", "b3", "b4"],
        "coordinates": {"lat": 45.85, "lon": -84.6},
        "depth_estimate_m": 120.0,
        "primary": {"material": "ferrous_composite", "certainty": 0.72, "backend": "tract_cpu"},
    },
    {
        "tile_id": "disagree_natural",
        "thermal_timeseries": ["b1", "b2", "b3", "b4", "b5", "b6"],
        "coordinates": {"lat": 45.85, "lon": -84.6},
        "depth_estimate_m": 80.0,
        "primary": {"material": "natural", "certainty": 0.4, "backend": "tract_cpu"},
    },
    {
        "tile_id": "sparse",
        "thermal_timeseries": ["b1"],
        "coordinates": {"lat": 42.0, "lon": -87.0},
        "depth_estimate_m": 30.0,
        "primary": {"material": "natural", "certainty": 0.55, "backend": "tract_cpu"},
    },
]


class ModelRunner:
    def __init__(self, path: str, mode: str) -> None:
        self.path = path
        self.mode = mode
        self.interpreter = None
        import tflite_runtime.interpreter as tflite  # type: ignore

        self.interpreter = tflite.Interpreter(model_path=path)
        self.interpreter.allocate_tensors()

    def certainty(self, features: list[float]) -> float:
        import numpy as np

        inp = self.interpreter.get_input_details()[0]
        out = self.interpreter.get_output_details()[0]
        scale, zp = inp.get("quantization", (0.0, 0))
        if scale and scale > 0:
            arr = np.clip(
                np.round(np.array(features, dtype=np.float32) / scale + zp),
                -128,
                127,
            ).astype(np.int8)
            self.interpreter.set_tensor(inp["index"], arr.reshape(1, 8))
        else:
            self.interpreter.set_tensor(
                inp["index"], np.array(features, dtype=np.float32).reshape(1, 8)
            )
        self.interpreter.invoke()
        raw = self.interpreter.get_tensor(out["index"]).flatten()
        if self.mode == "logits" and raw.size >= 2:
            a, b = float(raw[0]), float(raw[1])
            m = max(a, b)
            ea, eb = pow(2.718281828, a - m), pow(2.718281828, b - m)
            return float(eb / (ea + eb))
        return float(max(0.0, min(1.0, float(raw[0]))))


def eval_model(name: str, certainty_fn) -> dict:
    agreements = []
    deltas = []
    lat_ms = []
    rows = []
    for req in FIXTURES:
        feats = featurize(req)
        t0 = time.perf_counter()
        c = certainty_fn(feats)
        lat_ms.append((time.perf_counter() - t0) * 1000.0)
        mat = "ferrous_composite" if c > 0.7 else "natural"
        p = req["primary"]
        agr, agreed = agreement_score(p["material"], float(p["certainty"]), mat, c)
        agreements.append(agr)
        deltas.append(abs(float(p["certainty"]) - c))
        rows.append(
            {
                "tile": req["tile_id"],
                "certainty": round(c, 3),
                "material": mat,
                "agreement": round(agr, 3),
                "agreed": agreed,
            }
        )
    lat_ms.sort()
    p99 = lat_ms[int(0.99 * (len(lat_ms) - 1))] if lat_ms else 0.0
    return {
        "model": name,
        "mean_agreement": round(sum(agreements) / len(agreements), 3),
        "mean_abs_delta_certainty": round(sum(deltas) / len(deltas), 3),
        "p99_ms_cpu": round(p99, 2),
        "tiles": rows,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--sigmoid", help="sigmoid [1,1] tflite path")
    ap.add_argument("--logits", help="logits [1,2] tflite path")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    results = [eval_model("stub_rule", stub_certainty)]
    if args.sigmoid:
        r = ModelRunner(args.sigmoid, "sigmoid")
        results.append(eval_model("sigmoid", r.certainty))
    if args.logits:
        r = ModelRunner(args.logits, "logits")
        results.append(eval_model("logits", r.certainty))

    if args.json:
        print(json.dumps(results, indent=2))
    else:
        print("Compare on fixtures (higher mean_agreement vs primary is not always 'better'):")
        for row in results:
            print(
                f"  {row['model']:12} mean_agreement={row['mean_agreement']} "
                f"mean_|Δc|={row['mean_abs_delta_certainty']} p99_ms={row['p99_ms_cpu']}"
            )
            for t in row["tiles"]:
                print(f"    {t['tile']}: c={t['certainty']} agr={t['agreement']} agreed={t['agreed']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
