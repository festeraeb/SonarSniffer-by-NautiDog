#!/usr/bin/env python3
"""
TPU Server - Coral Edge TPU Inference Service
Runs on Xenon server, accessible from laptop @ school

Endpoints:
  POST /infer - Glint/jitter detection
  POST /train - Model update (optional)
  GET  /health - Health check
"""

from flask import Flask, request, jsonify
import base64
from io import BytesIO
from PIL import Image
import numpy as np
import time
from pathlib import Path

# Try to import TFLite with TPU support
try:
    import tflite_runtime.interpreter as tflite
    TFLITE_AVAILABLE = True
except ImportError:
    try:
        import tensorflow.lite as tflite
        TFLITE_AVAILABLE = True
    except ImportError:
        TFLITE_AVAILABLE = False

app = Flask(__name__)

# Configuration
MODEL_PATH = Path(__file__).parent / "models" / "glint_jitter.tflite"
USE_TPU = True  # Set to False for testing without Coral USB

# Global interpreter
interpreter = None

def find_tpu_device():
    """Find Coral TPU device (PCIe or USB)"""
    import subprocess
    
    # Check for PCIe devices
    try:
        result = subprocess.run(['lspci', '-d', '1a6e:'], capture_output=True, text=True)
        if result.returncode == 0 and result.stdout.strip():
            print(f"✓ Found Coral PCIe device:")
            for line in result.stdout.strip().split('\n'):
                print(f"    {line}")
            return 'pcie'
    except:
        pass
    
    # Check for USB devices
    try:
        result = subprocess.run(['lsusb', '-d', '1a6e:'], capture_output=True, text=True)
        if result.returncode == 0 and result.stdout.strip():
            print(f"✓ Found Coral USB device:")
            for line in result.stdout.strip().split('\n'):
                print(f"    {line}")
            return 'usb'
    except:
        pass
    
    print("⚠ No Coral TPU device found")
    return None

def load_model():
    """Load TFLite model (with or without TPU)"""
    global interpreter
    
    if not MODEL_PATH.exists():
        print(f"⚠ Model not found: {MODEL_PATH}")
        print("  Using stub inference (always passes)")
        return False
    
    # Find TPU device
    tpu_type = find_tpu_device() if USE_TPU else None
    
    try:
        if tpu_type:
            # Load with Edge TPU delegate
            interpreter = tflite.Interpreter(
                model_path=str(MODEL_PATH),
                experimental_delegates=[tflite.load_delegate('libedgetpu.so.1')]
            )
            print(f"✓ Loaded model with TPU ({tpu_type}): {MODEL_PATH}")
        else:
            # Load without TPU (CPU only)
            interpreter = tflite.Interpreter(model_path=str(MODEL_PATH))
            print(f"✓ Loaded model (CPU only): {MODEL_PATH}")
        
        interpreter.allocate_tensors()
        return True
        
    except Exception as e:
        print(f"⚠ Failed to load model: {e}")
        print("  Falling back to stub inference")
        return False

def run_tpu_inference(image: Image.Image):
    """Run inference on Coral TPU"""
    if interpreter is None:
        return None
    
    # Get input/output details
    input_details = interpreter.get_input_details()
    output_details = interpreter.get_output_details()
    
    # Preprocess image
    img_array = np.array(image.resize((224, 224)), dtype=np.float32) / 255.0
    if len(img_array.shape) == 2:
        img_array = np.stack([img_array] * 3, axis=-1)  # Grayscale to RGB
    img_array = np.expand_dims(img_array, axis=0)  # Add batch dimension
    
    # Set input
    interpreter.set_tensor(input_details[0]['index'], img_array)
    
    # Run inference
    interpreter.invoke()
    
    # Get output
    output = interpreter.get_tensor(output_details[0]['index'])
    
    # Parse output (assumes model outputs [glint_score, jitter_score])
    return {
        'glint_score': float(output[0][0]),
        'jitter_score': float(output[0][1]) if len(output[0]) > 1 else 0.0
    }

def stub_inference(image: Image.Image):
    """Stub inference (no model, just brightest pixels)"""
    # Convert to grayscale
    arr = np.array(image.convert('L'), dtype=np.float32)
    
    # Simple metrics
    brightness = float(np.mean(arr))  # Convert to Python float
    contrast = float(np.std(arr))
    
    # Heuristic: high brightness + low contrast = potential glint
    glint_score = float(min(1.0, brightness / 255.0 * (1.0 - contrast / 100.0)))
    
    # Jitter is hard to detect without model, default low
    jitter_score = 0.05
    
    return {
        'glint_score': glint_score,
        'jitter_score': jitter_score
    }

@app.route('/infer', methods=['POST'])
def infer():
    """
    Glint/jitter detection endpoint
    
    Input:
    {
        "image_base64": "<PNG base64 string>",
        "meta": {
            "tile_id": "S2C_16TDN_...",
            "band": "B11",
            "timestamp": "2025-09-16T10:30:00Z"
        }
    }
    
    Output:
    {
        "glint_score": 0.12,
        "jitter_score": 0.05,
        "pass": true,
        "took_ms": 5.2,
        "used_tpu": true
    }
    """
    start_time = time.time()
    
    # Parse request
    data = request.get_json()
    if not data or 'image_base64' not in data:
        return jsonify({'error': 'missing image_base64'}), 400
    
    meta = data.get('meta', {})
    
    # Decode image
    try:
        img_bytes = base64.b64decode(data['image_base64'])
        img = Image.open(BytesIO(img_bytes))
    except Exception as e:
        return jsonify({'error': f'failed to decode image: {e}'}), 400
    
    # Run inference
    if interpreter is not None:
        result = run_tpu_inference(img)
        used_tpu = True
    else:
        result = stub_inference(img)
        used_tpu = False
    
    # Determine pass/fail
    passed = result['glint_score'] < 0.5 and result['jitter_score'] < 0.5
    
    # Build response
    elapsed_ms = (time.time() - start_time) * 1000
    
    response = {
        **result,
        'pass': passed,
        'took_ms': round(elapsed_ms, 2),
        'used_tpu': used_tpu,
        'meta': meta
    }
    
    return jsonify(response)

@app.route('/train', methods=['POST'])
def train():
    """
    Model update endpoint (optional, for future federated learning)
    
    For now, just acknowledges receipt
    """
    data = request.get_json()
    if not data:
        return jsonify({'error': 'missing data'}), 400
    
    # In future: receive model updates, retrain, etc.
    
    return jsonify({
        'status': 'received',
        'message': 'Training endpoint not yet implemented'
    })

@app.route('/health', methods=['GET'])
def health():
    """Health check endpoint"""
    return jsonify({
        'status': 'healthy',
        'tpu_available': TFLITE_AVAILABLE,
        'model_loaded': interpreter is not None,
        'use_tpu': USE_TPU
    })

@app.route('/', methods=['GET'])
def index():
    """Root endpoint - API info"""
    return jsonify({
        'service': 'CESAROPS TPU Server',
        'version': '1.0.0',
        'endpoints': {
            'POST /infer': 'Glint/jitter detection',
            'POST /train': 'Model update (future)',
            'GET /health': 'Health check'
        }
    })

if __name__ == '__main__':
    print("="*70)
    print("CESAROPS TPU SERVER")
    print("="*70)
    
    # Load model
    model_ok = load_model()
    
    print()
    print(f"TPU Enabled: {USE_TPU}")
    print(f"Model Loaded: {model_ok}")
    print(f"TFLite Available: {TFLITE_AVAILABLE}")
    print()
    print("Starting server on http://0.0.0.0:5001")
    print()
    print("Endpoints:")
    print("  POST http://0.0.0.0:5001/infer  - Glint/jitter detection")
    print("  GET  http://0.0.0.0:5001/health - Health check")
    print("="*70)
    
    # Run server (0.0.0.0 = accessible from network)
    app.run(host='0.0.0.0', port=5001, debug=False)
