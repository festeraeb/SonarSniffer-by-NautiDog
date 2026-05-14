"""
TPU Inference Server — CESAROPS Coral Edge TPU Gateway

Minimal Flask server that accepts POST /infer with JSON:
  { "image_base64": "...PNG base64...", "meta": { "crs": "EPSG:4326", ... } }

Returns JSON detections:
  { "detections": [ {"row":.., "col":.., "score":..}, ... ] }

Runs on Xenon (with local TPU) or laptop (CPU stub fallback).

To run:
    python -m pip install Flask Pillow numpy
    python tpu_server.py

When Edge TPU is available, replace `run_inference` with pycoral/tflite-runtime.
"""
from flask import Flask, request, jsonify
import base64
from io import BytesIO
from PIL import Image
import numpy as np
import time
import sys
import logging
import threading

# ── Logging Setup ─────────────────────────────────────────────────────────────
logging.basicConfig(
    level=logging.INFO,
    format='[%(asctime)s] %(levelname)s | %(name)s | %(message)s',
    datefmt='%H:%M:%S'
)
log = logging.getLogger("tpu_server")

app = Flask(__name__)

# ── TPU detection ─────────────────────────────────────────────────────────────

TPU_AVAILABLE = False
TFLITE_RUNTIME = False
tflite = None

# Hardware lock to prevent concurrent TPU hardware calls from scrambling state
tpu_lock = threading.Lock()

try:
    import tflite_runtime.interpreter as tflite
    TFLITE_RUNTIME = True
except ImportError:
    try:
        # ai-edge-litert is Google's successor to tflite-runtime (Python 3.12+)
        import ai_edge_litert.interpreter as tflite
        TFLITE_RUNTIME = True
    except ImportError:
        log.warning("No TFLite runtime found. Running in simulation mode ONLY.")

# Detect Edge TPU by attempting to load the delegate directly.
if TFLITE_RUNTIME and tflite is not None:
    try:
        _tpu_lib = "edgetpu.dll" if sys.platform == "win32" else "libedgetpu.so.1.0"
        _test_delegate = tflite.load_delegate(_tpu_lib)
        del _test_delegate
        TPU_AVAILABLE = True
        log.info("Edge TPU hardware detected successfully.")
    except Exception as _tpu_err:
        log.warning(f"Edge TPU delegate not available: {_tpu_err}")


def _get_interpreter(model_path="models/glint_jitter_edgetpu.tflite"):
    """Load Edge TPU or CPU interpreter."""
    if TPU_AVAILABLE and TFLITE_RUNTIME:
        return tflite.Interpreter(
            model_path=model_path,
            experimental_delegates=[
                tflite.load_delegate("edgetpu.dll" if sys.platform == "win32"
                                     else "libedgetpu.so.1.0")
            ],
        )
    elif TFLITE_RUNTIME:
        return tflite.Interpreter(model_path=model_path.replace("_edgetpu", "_cpu"))
    else:
        return None


# Lazy-load interpreter
_interpreter = None


def get_interpreter():
    global _interpreter
    if _interpreter is None:
        try:
            _interpreter = _get_interpreter()
        except Exception:
            _interpreter = None
    return _interpreter


def run_inference(image: Image.Image, meta: dict):
    """
    Stub inference: locate brightest pixels as dummy glint/jitter events.

    Replace this body with real TFLite/Edge TPU code when model is available:

        interpreter = get_interpreter()
        interpreter.allocate_tensors()
        input_idx = interpreter.get_input_details()[0]["index"]
        output_idx = interpreter.get_output_details()[0]["index"]
        arr = np.array(image.convert('L'), dtype=np.uint8)
        interpreter.set_tensor(input_idx, arr.reshape(1, *arr.shape, 1))
        interpreter.invoke()
        output = interpreter.get_tensor(output_idx)
        return parse_output(output)
    """
    arr = np.array(image.convert('L'), dtype=np.float32)
    # Simple local maxima above threshold (brightest = specular glint candidates)
    thresh = np.nanpercentile(arr, 99.5)
    ys, xs = np.where(arr >= thresh)
    detections = []
    for y, x in zip(ys, xs):
        detections.append({
            'row': int(y),
            'col': int(x),
            'score': float(arr[y, x]) / 255.0,
        })
    # Limit
    detections = detections[:100]
    return detections


# ── Routes ────────────────────────────────────────────────────────────────────

@app.route('/infer', methods=['POST'])
def infer():
    start = time.time()
    try:
        data = request.get_json(silent=True)
        if not data or 'image_base64' not in data:
            log.error("Rejecting request: Missing image_base64 payload")
            return jsonify({'error': 'missing image_base64'}), 400

        b64 = data['image_base64']
        meta = data.get('meta', {})

        try:
            img_bytes = base64.b64decode(b64)
            img = Image.open(BytesIO(img_bytes)).copy()
        except Exception as e:
            log.error(f"Image decode failed: {e}")
            return jsonify({'error': f'failed to decode image: {repr(e)}'}), 400

        with tpu_lock:
            # Acquiring hardware lock for inference to avoid TPU bus collisions
            detections = run_inference(img, meta)

        elapsed = time.time() - start
        resp = {
            'detections': detections,
            'meta': meta,
            'took_s': round(elapsed, 4),
            'used_tpu': TPU_AVAILABLE,
        }
        log.info(f"Processed /infer in {elapsed:.3f}s. {len(detections)} detections found.")
        return jsonify(resp)

    except Exception as e:
        log.exception(f"Unhandled error during inference routing: {e}")
        return jsonify({'error': 'Internal server execution error', 'details': str(e)}), 500


@app.route('/health', methods=['GET'])
def health():
    return jsonify({
        'status': 'healthy',
        'tpu_available': TPU_AVAILABLE,
        'tflite_runtime': TFLITE_RUNTIME,
        'model_loaded': get_interpreter() is not None,
    })


# ── Main ──────────────────────────────────────────────────────────────────────

if __name__ == '__main__':
    import argparse
    p = argparse.ArgumentParser()
    p.add_argument('--port', type=int, default=5001)
    p.add_argument('--host', default='0.0.0.0')
    p.add_argument('--dev', action='store_true', help="Run utilizing Flask Dev server instead of Waitress")
    args = p.parse_args()

    log.info(f"Setting up TPU Server (Hardware Available: {TPU_AVAILABLE}, TFLite: {TFLITE_RUNTIME})")
    
    if args.dev:
        log.warning("Starting in Development mode. Do not use in production.")
        app.run(host=args.host, port=args.port)
    else:
        try:
            from waitress import serve
            log.info(f"Starting robust Waitress WSGI server on {args.host}:{args.port}")
            app.logger.handlers.clear()
            serve(app, host=args.host, port=args.port, threads=4, channel_timeout=60, cleanup_interval=30)
        except ImportError:
            log.warning("Waitress WSGI absent... Falling back to Flask dev server (pip install waitress for stability)")
            app.run(host=args.host, port=args.port)
