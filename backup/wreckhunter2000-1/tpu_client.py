#!/usr/bin/env python3
"""
TPU Client - Remote Coral TPU Access
Use T440's inference endpoint from anywhere via Cloudflare tunnel.
i7 RETIRED (2026-05-06) — all services now on T440 via cesarops.org tunnel.
"""

import requests
from requests.adapters import HTTPAdapter
from requests.packages.urllib3.util.retry import Retry
import base64
from io import BytesIO
from PIL import Image
from pathlib import Path
import logging

log = logging.getLogger("tpu_client")

TUNNEL_URL     = "https://llm.cesarops.org"      # Cloudflare tunnel (primary)
T440_LAN_URL   = "http://10.0.0.61:5001"         # T440 LAN direct
LOCAL_TPU_URL  = "http://localhost:5001"
_FALLBACK_URLS = [TUNNEL_URL, T440_LAN_URL, LOCAL_TPU_URL]


def create_tpu_client(prefer_xeon: bool = True) -> "TPUClient":
    """
    Return a TPUClient connected to the first reachable TPU server.
    Order: Cloudflare tunnel -> T440 LAN -> localhost:5001 -> CPU stub.
    """
    urls = _FALLBACK_URLS if prefer_xeon else list(reversed(_FALLBACK_URLS))
    for url in urls:
        client = TPUClient(url)
        h = client.health_check()
        if h.get("status") == "healthy":
            return client
    # Nothing reachable — return client that will use CPU stub health path
    return TPUClient(LOCAL_TPU_URL)


class TPUClient:
    """Client for remote TPU inference"""
    
    def __init__(self, server_url=LOCAL_TPU_URL, retries=3, backoff_factor=0.3):
        """
        Initialize TPU client with automated, robust retries.

        Args:
            server_url: TPU server URL (use create_tpu_client() for auto-discovery)
            retries: Number of connection/read retries
            backoff_factor: Backoff factor for retries
        """
        self.server_url = server_url
        self._warned_unreachable = False  # suppress repeated connection-refused spam
        self._unreachable = False         # skip network entirely after first failure
        
        # Configure robust connection pooling and retries
        self._session = requests.Session()
        retry_strategy = Retry(
            total=retries,
            backoff_factor=backoff_factor,
            status_forcelist=[429, 500, 502, 503, 504],
            allowed_methods=["POST", "GET"]
        )
        adapter = HTTPAdapter(max_retries=retry_strategy, pool_connections=10, pool_maxsize=10)
        self._session.mount("http://", adapter)
        self._session.mount("https://", adapter)
    
    def check_glint_jitter(self, image, meta=None):
        """
        Check image for glint and jitter
        
        Args:
            image: PIL Image, numpy array, or file path
            meta: Optional metadata dict
        
        Returns:
            dict: {
                'glint_score': float (0-1),
                'jitter_score': float (0-1),
                'pass': bool,
                'took_ms': float,
                'used_tpu': bool
            }
        """
        # Convert image to base64
        img_bytes = self._image_to_bytes(image)

        # If server already known unreachable, skip the network call entirely.
        # On Windows, requests to a closed port can take ~4 s each; with thousands
        # of detections that would take hours.
        if self._unreachable:
            return self._cpu_fallback_inference(image)

        b64 = base64.b64encode(img_bytes).decode('utf-8')
        
        # Build request
        payload = {
            'image_base64': b64,
            'meta': meta or {}
        }
        
        # Send to server via robust session
        try:
            response = self._session.post(
                f"{self.server_url}/infer",
                json=payload,
                timeout=(5.0, 30.0) # 5s connect timeout, 30s read timeout
            )
            response.raise_for_status()
            return response.json()
            
        except requests.exceptions.ConnectionError as ce:
            # Server completely unreachable — mark so all future calls skip the network hop
            self._unreachable = True
            if not self._warned_unreachable:
                log.warning(f"TPU server unreachable ({ce}) — falling back to CPU stub.")
                self._warned_unreachable = True
            return self._cpu_fallback_inference(image)
        
        except requests.exceptions.Timeout as te:
            log.warning(f"TPU inference timed out: {te} — falling back to CPU stub")
            return self._cpu_fallback_inference(image)
            
        except Exception as e:
            log.error(f"TPU inference failed: {e} — falling back to CPU stub")
            return self._cpu_fallback_inference(image)

    def _cpu_fallback_inference(self, image) -> dict:
        """
        Pure-Python CPU stub inference used when no TPU/Flask server is available.
        Performs the same local-maxima glint check as tpu_server.py's run_inference().
        """
        try:
            import numpy as np
            img_bytes = self._image_to_bytes(image)
            from PIL import Image as _PIL
            from io import BytesIO as _BytesIO
            arr = np.array(_PIL.open(_BytesIO(img_bytes)).convert('L'), dtype=np.float32)
            thresh = float(np.nanpercentile(arr, 99.5))
            bright_pct = float(np.mean(arr >= thresh))
            glint_score = min(bright_pct * 20.0, 1.0)   # scale 0.5% bright pixels → 0.1 score
            # Jitter: measure local gradient variance as proxy
            gy = arr[1:, :] - arr[:-1, :]
            gx = arr[:, 1:] - arr[:, :-1]
            jitter_score = min(float(np.std(gy)) / 64.0, 1.0)
            return {
                'glint_score': round(glint_score, 4),
                'jitter_score': round(jitter_score, 4),
                'pass': glint_score < 0.5 and jitter_score < 0.5,
                'took_ms': 0,
                'used_tpu': False,
                'backend': 'cpu_stub',
            }
        except Exception as e:
            return {
                'glint_score': 0.0,
                'jitter_score': 0.0,
                'pass': True,
                'took_ms': 0,
                'used_tpu': False,
                'backend': 'cpu_stub_failed',
                'error': str(e),
            }
    
    def health_check(self):
        """Check if TPU server is online with tight timeout"""
        try:
            response = self._session.get(f"{self.server_url}/health", timeout=(2.0, 5.0))
            response.raise_for_status()
            return response.json()
        except requests.exceptions.RequestException as e:
            return {
                'status': 'unreachable',
                'error': f"TPU unreachable: {str(e)}"
            }
    
    def _image_to_bytes(self, image):
        """Convert image to PNG bytes"""
        if isinstance(image, (str, Path)):
            # File path
            img = Image.open(image)
        elif isinstance(image, Image.Image):
            # Already PIL Image
            img = image
        elif hasattr(image, 'shape'):
            # Numpy array
            img = Image.fromarray(image)
        else:
            raise ValueError(f"Unknown image type: {type(image)}")
        
        # Convert to PNG bytes
        buf = BytesIO()
        img.save(buf, format='PNG')
        return buf.getvalue()


# Convenience function for quick use
def quick_glint_check(image_path, server_url="http://localhost:5001"):
    """
    Quick glint/jitter check
    
    Usage:
        result = quick_glint_check("tile.png")
        if result['pass']:
            print("Tile is good!")
    """
    client = TPUClient(server_url)
    return client.check_glint_jitter(image_path)


# Test function
def test_tpu_server(server_url="http://localhost:5001"):
    """Test TPU server with sample image"""
    print("="*70)
    print("TPU CLIENT TEST")
    print("="*70)
    
    client = TPUClient(server_url)
    
    # Health check
    print("\n[1/2] Health check...")
    health = client.health_check()
    print(f"  Status: {health.get('status', 'unknown')}")
    print(f"  TPU Available: {health.get('tpu_available', False)}")
    print(f"  Model Loaded: {health.get('model_loaded', False)}")
    
    # Test inference (create simple test image)
    print("\n[2/2] Test inference...")
    test_img = Image.new('L', (224, 224), color=128)  # Gray square
    
    result = client.check_glint_jitter(
        test_img,
        meta={'test': True, 'source': 'tpu_client_test'}
    )
    
    print(f"  Glint Score: {result['glint_score']:.3f}")
    print(f"  Jitter Score: {result['jitter_score']:.3f}")
    print(f"  PASS: {result['pass']}")
    print(f"  Took: {result['took_ms']:.2f}ms")
    print(f"  Used TPU: {result['used_tpu']}")
    
    if 'error' in result:
        print(f"  Error: {result['error']}")
    
    print("\n" + "="*70)
    
    return result


if __name__ == "__main__":
    import sys
    
    # Default: test local server
    server_url = "http://localhost:5001"
    
    # Override with command line arg
    if len(sys.argv) > 1:
        server_url = sys.argv[1]
    
    print(f"\nTesting TPU server: {server_url}")
    print("Make sure tpu_server.py is running on that address\n")
    
    test_tpu_server(server_url)
