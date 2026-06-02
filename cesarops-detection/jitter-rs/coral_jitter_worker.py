#!/usr/bin/env python3
"""
Coral Edge TPU jitter validator — ML350e worker (CORAL_TPU_WORKER_SPEC.md).

  GET  /health
  POST /validate

Env: CORAL_PORT, CORAL_MODEL, FLEET_KEY, CORAL_OUTPUT (sigmoid|logits)
"""
from __future__ import annotations

import json
import logging
import os
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import urlparse

LOG = logging.getLogger("coral-jitter")
MATERIAL_THRESHOLD = 0.7


def clamp(x: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, x))


def featurize(req: dict[str, Any]) -> list[float]:
    """8-element vector — must match jitter-rs inference.rs featurize()."""
    bands = req.get("thermal_timeseries") or []
    n = float(len(bands))
    coords = req.get("coordinates") or {}
    lat = float(coords.get("lat", 0.0))
    lon = float(coords.get("lon", 0.0))
    depth = float(req.get("depth_estimate_m", 0.0))
    return [
        n,
        min(n, 6.0),
        lat / 90.0,
        lon / 180.0,
        depth / 500.0,
        1.0 if depth > 100.0 else 0.0,
        1.0 if n >= 2.0 else 0.0,
        1.0,
    ]


def agreement_score(primary_mat: str, primary_cert: float, material: str, certainty: float) -> tuple[float, bool]:
    if primary_mat == material:
        delta = abs(primary_cert - certainty)
        return clamp(1.0 - delta, 0.0, 1.0), True
    return clamp(1.0 - certainty, 0.0, 0.5) * 0.5, False


def stub_certainty(features: list[float]) -> float:
    n = int(features[0])
    bands = min(n, 6)
    return min(0.6 + 0.06 * bands, 0.92)


class CoralEngine:
    """Edge TPU / CPU tflite when available; else spec §8 stub rule."""

    def __init__(self) -> None:
        self.tpu_present = self._detect_tpu()
        self.model_path = os.environ.get(
            "CORAL_MODEL", "/opt/cesarops/models/jitter_edgetpu.tflite"
        )
        self.output_mode_env = os.environ.get("CORAL_OUTPUT", "auto").lower()
        self.output_mode = "sigmoid"
        self.output_shape: list[int] = []
        self.interpreter = None
        self.backend = "stub_rule"
        self._load_model()

    @staticmethod
    def _detect_tpu() -> bool:
        if os.path.isdir("/dev") and any(
            n.startswith("apex_") for n in os.listdir("/dev")
        ):
            return True
        try:
            import usb.core  # type: ignore

            for vid, pid in ((0x1A6E, 0x089A), (0x18D1, 0x9302)):
                if usb.core.find(idVendor=vid, idProduct=pid) is not None:
                    return True
        except Exception:
            pass
        return False

    def _load_model(self) -> None:
        path = self.model_path
        if not os.path.isfile(path):
            LOG.warning("model missing %s — using stub_rule", path)
            return
        try:
            from pycoral.utils import edgetpu  # type: ignore
            from pycoral.adapters import common  # type: ignore

            self.interpreter = edgetpu.make_interpreter(path)
            self.interpreter.allocate_tensors()
            self.backend = "edgetpu_int8"
            LOG.info("loaded Edge TPU model %s", path)
            self._finalize_output_mode()
            return
        except Exception as e:
            LOG.warning("pycoral edgetpu load failed: %s", e)
        try:
            import tflite_runtime.interpreter as tflite  # type: ignore

            self.interpreter = tflite.Interpreter(model_path=path)
            self.interpreter.allocate_tensors()
            self.backend = "tflite_cpu"
            LOG.info("loaded CPU tflite %s", path)
        except Exception as e:
            LOG.warning("tflite load failed: %s — stub_rule", e)
        self._finalize_output_mode()

    def _finalize_output_mode(self) -> None:
        if self.interpreter is None:
            self.output_mode = "stub"
            self.output_shape = []
            return
        out = self.interpreter.get_output_details()[0]
        shape = [int(x) for x in out.get("shape", [])]
        self.output_shape = shape
        n = 1
        for d in shape:
            if d > 0:
                n *= d
        detected = "logits" if n >= 2 else "sigmoid"
        if self.output_mode_env in ("sigmoid", "logits"):
            self.output_mode = self.output_mode_env
        else:
            self.output_mode = detected
        LOG.info(
            "output_mode=%s (env=%s shape=%s elements=%d)",
            self.output_mode,
            self.output_mode_env,
            shape,
            n,
        )

    def _certainty_from_output(self, raw) -> float:
        import numpy as np

        flat = np.asarray(raw).flatten()
        if self.output_mode == "logits" and flat.size >= 2:
            a, b = float(flat[0]), float(flat[1])
            m = max(a, b)
            ea, eb = pow(2.718281828, a - m), pow(2.718281828, b - m)
            return float(eb / (ea + eb))
        return float(clamp(float(flat[0]), 0.0, 1.0))

    def infer(self, features: list[float]) -> float:
        if self.interpreter is None:
            return stub_certainty(features)
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
        t0 = time.perf_counter()
        self.interpreter.invoke()
        raw = self.interpreter.get_tensor(out["index"])
        _ = time.perf_counter() - t0
        return self._certainty_from_output(raw)

    def health(self) -> dict[str, Any]:
        loaded = self.interpreter is not None or self.backend == "stub_rule"
        ok = self.tpu_present and self.backend == "edgetpu_int8"
        status = "ok" if (loaded and (ok or self.backend != "edgetpu_int8")) else "degraded"
        if not self.tpu_present:
            status = "degraded"
        if self.backend == "stub_rule":
            status = "degraded"
        return {
            "service": "coral-jitter-validator",
            "device": "coral_edgetpu",
            "tpu_present": self.tpu_present,
            "model_loaded": loaded,
            "backend": self.backend,
            "status": status,
            "model_path": self.model_path,
            "output_mode": self.output_mode,
            "output_shape": self.output_shape,
        }


ENGINE = CoralEngine()
FLEET_KEY = os.environ.get("FLEET_KEY", "").strip()


class Handler(BaseHTTPRequestHandler):
    def _auth_ok(self) -> bool:
        if not FLEET_KEY:
            return True
        return self.headers.get("X-Fleet-Key", "") == FLEET_KEY

    def _json(self, code: int, obj: dict[str, Any]) -> None:
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        if urlparse(self.path).path != "/health":
            self._json(404, {"error": "not found"})
            return
        self._json(200, ENGINE.health())

    def do_POST(self) -> None:
        if urlparse(self.path).path != "/validate":
            self._json(404, {"error": "not found"})
            return
        if not self._auth_ok():
            self._json(401, {"error": "unauthorized"})
            return
        n = int(self.headers.get("Content-Length", 0))
        try:
            req = json.loads(self.rfile.read(n) or b"{}")
        except json.JSONDecodeError:
            self._json(400, {"error": "invalid json"})
            return
        tile_id = str(req.get("tile_id", ""))
        t0 = time.perf_counter()
        try:
            feats = featurize(req)
            certainty = clamp(ENGINE.infer(feats), 0.0, 1.0)
            material = (
                "ferrous_composite" if certainty > MATERIAL_THRESHOLD else "natural"
            )
            primary = req.get("primary") or {}
            pmat = str(primary.get("material", ""))
            pcert = float(primary.get("certainty", 0.0))
            agreement, agreed = agreement_score(pmat, pcert, material, certainty)
            infer_ms = round((time.perf_counter() - t0) * 1000.0, 1)
            LOG.info(
                "tile=%s material=%s certainty=%.3f agreed=%s agreement=%.3f infer_ms=%s backend=%s",
                tile_id,
                material,
                certainty,
                agreed,
                agreement,
                infer_ms,
                ENGINE.backend,
            )
            self._json(
                200,
                {
                    "device": "coral_edgetpu",
                    "backend": ENGINE.backend,
                    "material": material,
                    "certainty": round(certainty, 3),
                    "agreement": round(agreement, 3),
                    "agreed": agreed,
                    "infer_ms": infer_ms,
                },
            )
        except Exception as e:
            LOG.exception("validate %s: %s", tile_id, e)
            self._json(
                200,
                {
                    "device": "coral_edgetpu",
                    "backend": ENGINE.backend,
                    "material": "natural",
                    "certainty": 0.0,
                    "agreement": 0.0,
                    "agreed": False,
                    "infer_ms": round((time.perf_counter() - t0) * 1000.0, 1),
                    "error": str(e)[:200],
                },
            )

    def log_message(self, fmt: str, *args: Any) -> None:
        LOG.info("%s - %s", self.address_string(), fmt % args)


def main() -> None:
    logging.basicConfig(
        level=os.environ.get("LOG_LEVEL", "INFO"),
        format="%(asctime)s %(levelname)s %(message)s",
    )
    port = int(os.environ.get("CORAL_PORT", "8190"))
    host = os.environ.get("CORAL_HOST", "0.0.0.0")
    LOG.info("coral-jitter-validator %s:%s %s", host, port, ENGINE.health())
    ThreadingHTTPServer((host, port), Handler).serve_forever()


if __name__ == "__main__":
    main()
