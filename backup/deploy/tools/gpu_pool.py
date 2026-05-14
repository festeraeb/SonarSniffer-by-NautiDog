#!/usr/bin/env python3
"""
GPU Pool — Registry + Client for CESAROPS GPU Fleet

Discovers GPU servers on the Tailscale mesh (and LAN), routes jobs to the
first idle GPU, falls back to CPU.

Usage:
    from gpu_pool import create_gpu_client
    client = create_gpu_client()
    result = client.infer(tile_image, meta={"tile_id": "t001"})

Registry:
    pool = GPURegistry()
    pool.discover()           # scan known + Tailscale nodes
    pool.status()             # {node: health_info, ...}
    pool.pick_idle()          # -> GPUClient pointed at idle node
"""

import json
import os
import subprocess
import time
from pathlib import Path
from io import BytesIO
from typing import Optional
import base64

import requests
from PIL import Image

# ── Known GPU nodes (Tailscale IPs are stable) ───────────────────────────────

# Format: (tailscale_ip, gpu_port, lan_ip, description)
_KNOWN_NODES = [
    ("68.55.42.202", 5002, "10.0.0.56", "i7 / Gigabyte GPU + Coral TPU"),       
    ("100.110.214.86", 5002, None, "Laptop / M2200"),
    (None, 5002, "100.105.77.74", "New Node / Quadro P1000"),
    (None, 5002, "10.0.0.61", "T440 / dual-P100 overflow"),
    (None, 5002, "10.0.0.204", "GTX 1060 node"),
    # Future Optiplex 7010s — add here when online:
    # ("100.x.x.x", 5002, "10.0.0.x", "Optiplex-1"),
    # ("100.x.x.x", 5002, "10.0.0.x", "Optiplex-2"),
]

GPU_PORT = int(os.environ.get("GPU_PORT", "5002"))


# ── GPUClient ─────────────────────────────────────────────────────────────────

class GPUClient:
    """Client for a single GPU server (mirrors TPUClient pattern)."""

    def __init__(self, server_url: str):
        self.server_url = server_url.rstrip("/")

    def infer(self, image, meta: dict = None, threshold: float = 2.5) -> dict:
        """Send image to GPU server, return anomaly results."""
        img_bytes = self._image_to_bytes(image)
        b64 = base64.b64encode(img_bytes).decode("utf-8")

        payload = {
            "image_base64": b64,
            "meta": meta or {},
            "threshold": threshold,
        }
        try:
            resp = requests.post(
                f"{self.server_url}/gpu/infer", json=payload, timeout=30
            )
            if resp.status_code == 503:
                return {"error": "gpu_busy", "server": self.server_url}
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.ConnectionError:
            return self._cpu_fallback(image, threshold)
        except Exception as e:
            print(f"⚠ GPU infer failed ({self.server_url}): {e}")
            return self._cpu_fallback(image, threshold)

    def health(self) -> dict:
        """Check GPU server health."""
        try:
            resp = requests.get(f"{self.server_url}/gpu/health", timeout=5)
            return resp.json()
        except Exception:
            return {"status": "unreachable", "server": self.server_url}

    def is_idle(self) -> bool:
        h = self.health()
        return h.get("status") == "healthy" and not h.get("busy", True)

    def _cpu_fallback(self, image, threshold: float = 2.5) -> dict:
        """NumPy z-score fallback when no GPU server is reachable."""
        import numpy as np

        arr = np.array(
            Image.open(BytesIO(self._image_to_bytes(image))).convert("L"),
            dtype=np.float32,
        )
        mean_val = np.mean(arr)
        std_val = np.std(arr)
        zscore = (arr - mean_val) / (std_val + 1e-6)
        anomaly_count = int(np.sum(np.abs(zscore) > threshold))
        return {
            "anomaly_count": anomaly_count,
            "mean": round(float(mean_val), 4),
            "std": round(float(std_val), 4),
            "max_zscore": round(float(np.max(np.abs(zscore))), 4),
            "top_hits": [],
            "gpu_used": False,
            "backend": "cpu_fallback",
        }

    @staticmethod
    def _image_to_bytes(image) -> bytes:
        if isinstance(image, (str, Path)):
            img = Image.open(image)
        elif isinstance(image, Image.Image):
            img = image
        elif hasattr(image, "shape"):
            img = Image.fromarray(image)
        else:
            raise ValueError(f"Unknown image type: {type(image)}")
        buf = BytesIO()
        img.save(buf, format="PNG")
        return buf.getvalue()


# ── GPURegistry ───────────────────────────────────────────────────────────────

class GPURegistry:
    """Discover and track GPU servers across the Tailscale mesh."""

    def __init__(self):
        self.nodes: dict[str, dict] = {}  # url -> health info

    def discover(self, include_tailscale: bool = True) -> dict:
        """
        Scan known nodes + Tailscale peers for /gpu/health endpoints.
        Returns {url: health_dict}.
        """
        urls_to_check = set()

        # 1) Known nodes
        for ts_ip, port, lan_ip, desc in _KNOWN_NODES:
            urls_to_check.add(f"http://{ts_ip}:{port}")
            if lan_ip:
                urls_to_check.add(f"http://{lan_ip}:{port}")

        # 2) Dynamic Tailscale peers
        if include_tailscale:
            for ip in self._tailscale_peers():
                urls_to_check.add(f"http://{ip}:{GPU_PORT}")

        # 3) Localhost
        urls_to_check.add(f"http://localhost:{GPU_PORT}")

        # Probe all
        self.nodes = {}
        for url in urls_to_check:
            client = GPUClient(url)
            h = client.health()
            if h.get("status") == "healthy":
                self.nodes[url] = h
        return self.nodes

    def status(self) -> dict:
        """Return cached node health (call discover() first or to refresh)."""
        return self.nodes

    def pick_idle(self) -> Optional[GPUClient]:
        """Return a GPUClient pointed at the first idle GPU, or None."""
        for url, info in self.nodes.items():
            if not info.get("busy", True):
                return GPUClient(url)
        return None

    def pick_best(self) -> Optional[GPUClient]:
        """Pick idle GPU with most VRAM, or None."""
        best_url, best_vram = None, -1
        for url, info in self.nodes.items():
            if not info.get("busy", True):
                vram = info.get("vram_free_mb", info.get("gpu", {}).get("vram_mb", 0))
                if vram > best_vram:
                    best_url, best_vram = url, vram
        return GPUClient(best_url) if best_url else None

    @staticmethod
    def _tailscale_peers() -> list[str]:
        """Get Tailscale peer IPs from `tailscale status --json`."""
        try:
            result = subprocess.run(
                ["tailscale", "status", "--json"],
                capture_output=True, text=True, timeout=5,
            )
            if result.returncode != 0:
                return []
            data = json.loads(result.stdout)
            ips = []
            for peer in (data.get("Peer") or {}).values():
                if peer.get("Online") and peer.get("TailscaleIPs"):
                    # First IP is typically IPv4
                    ips.append(peer["TailscaleIPs"][0])
            return ips
        except Exception:
            return []


# ── Convenience ───────────────────────────────────────────────────────────────

def create_gpu_client() -> GPUClient:
    """
    Auto-discover GPU pool → return client pointed at first idle GPU.
    Falls back to CPU-only client if nothing found.
    """
    registry = GPURegistry()
    registry.discover()

    client = registry.pick_idle()
    if client:
        return client

    # Nothing idle — try any healthy node (might be busy, but worth a shot)
    for url in registry.nodes:
        return GPUClient(url)

    # Nothing at all — return localhost stub (will CPU fallback on infer)
    print("⚠ No GPU servers found — using CPU fallback")
    return GPUClient(f"http://localhost:{GPU_PORT}")


# ── CLI ───────────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    print("=" * 60)
    print("CESAROPS GPU POOL — Discovery")
    print("=" * 60)

    registry = GPURegistry()
    nodes = registry.discover()

    if not nodes:
        print("\n⚠ No GPU servers found on Tailscale mesh or LAN.")
        print("  Start gpu_server.py on a node: python gpu_server.py")
    else:
        print(f"\n✓ Found {len(nodes)} GPU server(s):\n")
        for url, info in nodes.items():
            gpu = info.get("gpu", {})
            busy = "BUSY" if info.get("busy") else "idle"
            name = gpu.get("name", "?")
            vram = info.get("vram_free_mb", gpu.get("vram_mb", "?"))
            jobs = info.get("jobs_completed", 0)
            print(f"  {url}")
            print(f"    GPU: {name}  VRAM free: {vram} MB  Status: {busy}  Jobs: {jobs}")
            print()

    # Test inference if any nodes available
    client = registry.pick_idle()
    if client:
        print("Testing inference on idle GPU...")
        test_img = Image.new("L", (256, 256), color=128)
        result = client.infer(test_img, meta={"test": True})
        print(f"  Anomalies: {result.get('anomaly_count', '?')}")
        print(f"  Backend: {result.get('backend', '?')}")
        print(f"  Took: {result.get('took_s', '?')}s")
    print("=" * 60)
