#!/usr/bin/env python3
"""Mock Coral Edge TPU validator implementing CORAL_TPU_WORKER_SPEC.md (stub model).

Reference implementation for integration testing only. The real ML350e worker
runs an int8 .tflite on the Edge TPU; this stub uses the §8 fallback rule.
"""
import json
from http.server import BaseHTTPRequestHandler, HTTPServer

def clamp(x, lo, hi):
    return max(lo, min(hi, x))

class H(BaseHTTPRequestHandler):
    def _send(self, code, obj):
        b = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        if self.path == "/health":
            self._send(200, {"service": "coral-jitter-validator", "device": "coral_edgetpu",
                             "tpu_present": True, "model_loaded": True,
                             "backend": "edgetpu_int8", "status": "ok"})
        else:
            self._send(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/validate":
            self._send(404, {"error": "not found"})
            return
        n = int(self.headers.get("Content-Length", 0))
        req = json.loads(self.rfile.read(n) or b"{}")
        bands = min(len(req.get("thermal_timeseries", [])), 6)
        certainty = min(0.6 + 0.06 * bands, 0.92)  # stub rule per spec §8
        material = "ferrous_composite" if certainty > 0.7 else "natural"
        primary = req.get("primary", {})
        pmat = primary.get("material", "")
        pcert = float(primary.get("certainty", 0.0))
        if material == pmat:
            agreement = clamp(1.0 - abs(pcert - certainty), 0.0, 1.0)
            agreed = True
        else:
            agreement = clamp(1.0 - certainty, 0.0, 0.5) * 0.5
            agreed = False
        self._send(200, {"device": "coral_edgetpu", "backend": "edgetpu_int8",
                         "material": material, "certainty": round(certainty, 3),
                         "agreement": round(agreement, 3), "agreed": agreed,
                         "infer_ms": 11.2})

    def log_message(self, *a):
        pass

if __name__ == "__main__":
    HTTPServer(("0.0.0.0", 8190), H).serve_forever()
