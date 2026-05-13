import os
import sys
import time
import json
import platform
import socket
import threading

try:
    import psutil
except ImportError:
    print("Error: psutil is required. Install with: pip install psutil")
    sys.exit(1)

try:
    import requests
except ImportError:
    print("Error: requests is required. Install with: pip install requests")
    sys.exit(1)

try:
    import GPUtil
    HAS_GPUTIL = True
except ImportError:
    HAS_GPUTIL = False

# Configuration via environment variables
API_URL = os.environ.get("WRECKS_API_URL", "http://127.0.0.1:8000")
WORKER_ID = os.environ.get("WRECKS_WORKER_ID", socket.gethostname())
POLL_INTERVAL = int(os.environ.get("TELEMETRY_INTERVAL", 5))

def get_gpu_stats():
    gpus = []
    if HAS_GPUTIL:
        try:
            for gpu in GPUtil.getGPUs():
                gpus.append({
                    "name": gpu.name,
                    "load": gpu.load * 100.0,
                    "memory_used": gpu.memoryUsed,
                    "memory_total": gpu.memoryTotal
                })
        except Exception as e:
            print(f"Error querying NVIDIA GPUs: {e}")
    return gpus

def collect_telemetry():
    # Initialize cpu_percent calculation if not already
    return {
        "worker_id": WORKER_ID,
        "platform": platform.platform(),
        "cpu_percent": psutil.cpu_percent(interval=None),
        "ram_percent": psutil.virtual_memory().percent,
        "gpus": get_gpu_stats(),
        "timestamp": time.time()
    }

def main():
    print(f"Starting hardware telemetry service for node: {WORKER_ID}")
    print(f"Target API: {API_URL}/telemetry")
    print(f"Polling Interval: {POLL_INTERVAL} seconds")
    
    if not HAS_GPUTIL:
        print("Note: GPUtil not installed. NVIDIA GPU metrics will not be collected.")
        print("      To enable: pip install gputil")

    # Prime the psutil cpu_percent internal timer
    psutil.cpu_percent(interval=None)
    time.sleep(1)

    while True:
        try:
            payload = collect_telemetry()
            resp = requests.post(f"{API_URL}/telemetry", json=payload, timeout=5)
            
            if resp.status_code != 200:
                print(f"[{time.strftime('%H:%M:%S')}] Warning: API returned status {resp.status_code}")
                
        except requests.exceptions.ConnectionError:
            print(f"[{time.strftime('%H:%M:%S')}] Connection error: Unable to reach {API_URL}")
        except Exception as e:
            print(f"[{time.strftime('%H:%M:%S')}] Telemetry push failed: {e}")
        
        time.sleep(POLL_INTERVAL)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nTelemetry service stopped by user.")
