#!/usr/bin/env python3
"""
Poll fleet health and write var/fleet-health/latest.json for n8n + Forge routing.

Merges:
  - GPU slot heartbeats (last known good llama per port)
  - Live HTTP probes (/v1/models, n8n, forge, mcp)
  - routing_state.json (desired roles)

n8n workers should read this file (or scripts/n8n_fleet_health_resolve.js) instead of hardcoded URLs.
"""
from __future__ import annotations

import json
import os
import subprocess
import time
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

import requests

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
OUT_DIR = REPO / "var/fleet-health"
OUT_FILE = OUT_DIR / "latest.json"
HEARTBEAT = Path(
    os.environ.get(
        "GPU_SLOT_HEARTBEAT_PATH",
        REPO / "var/gpu-slot-heartbeats/latest.json",
    )
)
ROUTING = Path(os.environ.get("FORGE_ROUTING_STATE", REPO / "cesarops-forge-v2/routing_state.json"))
T440 = os.environ.get("T440_LAN", "10.0.0.61")
C2 = os.environ.get("CESAROPS2_LAN_HOST", "10.0.0.201")
PATCH_ROUTING = os.environ.get("FLEET_HEALTH_PATCH_ROUTING", "1") == "1"

ROLE_KEYS = (
    "thinker_endpoint",
    "coder_endpoint",
    "reviewer_endpoint",
    "corrector_endpoint",
    "draft_endpoint",
    "validator_zaya",
)

DEFAULT_POOL: dict[str, list[str]] = {
    "thinker_endpoint": [
        f"http://{C2}:5203",
        f"http://{C2}:5200",
        f"http://127.0.0.1:5203",
        f"http://127.0.0.1:5200",
    ],
    "coder_endpoint": [
        f"http://{T440}:5001",
        f"http://127.0.0.1:5001",
    ],
    "reviewer_endpoint": [
        f"http://{T440}:5002",
        f"http://127.0.0.1:5002",
    ],
    "corrector_endpoint": [
        f"http://{T440}:5002",
        f"http://{C2}:5200",
    ],
    "draft_endpoint": [
        f"http://{C2}:5200",
        f"http://127.0.0.1:5200",
    ],
    "validator_zaya": [f"http://{C2}:5203", f"http://127.0.0.1:5203"],
}


def log(msg: str) -> None:
    print(f"[fleet-health] {msg}", flush=True)


def load_json(path: Path) -> dict[str, Any]:
    if path.is_file():
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            pass
    return {}


def models_ok(base: str, timeout: float = 4.0) -> bool:
    url = f"{base.rstrip('/')}/v1/models"
    try:
        r = requests.get(url, timeout=timeout)
        return r.ok
    except requests.RequestException:
        return False


def health_ok(url: str, timeout: float = 4.0) -> bool:
    try:
        r = requests.get(url, timeout=timeout)
        return r.ok
    except requests.RequestException:
        return False


def slot_to_base(slot: dict[str, Any]) -> str | None:
    host = slot.get("host") or "127.0.0.1"
    port = slot.get("port")
    if port is None:
        return None
    return f"http://{host}:{port}"


def heartbeat_bases(store: dict[str, Any]) -> dict[int, str]:
    out: dict[int, str] = {}
    for _key, slot in (store.get("slots") or {}).items():
        if not isinstance(slot, dict):
            continue
        if not slot.get("healthy"):
            continue
        base = slot_to_base(slot)
        port = slot.get("port")
        if base and port is not None:
            out[int(port)] = base
    return out


def pick_endpoint(role: str, desired: str, hb_ports: dict[int, str]) -> dict[str, Any]:
    pool: list[str] = []
    if desired:
        pool.append(desired.rstrip("/"))
    for u in DEFAULT_POOL.get(role, []):
        if u not in pool:
            pool.append(u)
    # Add heartbeat ports for thinker/draft on c2
    for port, base in sorted(hb_ports.items()):
        if port in (5200, 5201, 5202, 5203, 5571) and base not in pool:
            pool.append(base)

    for url in pool:
        if models_ok(url):
            return {"url": url, "source": "probe", "healthy": True}
    # Last heartbeat fallback (marked unhealthy for live probe)
    if desired:
        try:
            p = urlparse(desired)
            port = p.port
            if port and port in hb_ports:
                return {
                    "url": hb_ports[port],
                    "source": "heartbeat_last",
                    "healthy": False,
                }
        except Exception:
            pass
    return {"url": desired or pool[0] if pool else "", "source": "none", "healthy": False}


def main() -> int:
    # Refresh heartbeat store
    hb_script = REPO / "scripts/gpu_slot_heartbeat.py"
    if hb_script.is_file():
        subprocess.run(
            [os.environ.get("PYTHON", "python3"), str(hb_script), "tick", "--no-recover"],
            cwd=str(REPO),
            capture_output=True,
            timeout=120,
        )

    store = load_json(HEARTBEAT)
    routing = load_json(ROUTING)
    hb_ports = heartbeat_bases(store)

    endpoints: dict[str, Any] = {}
    for role in ROLE_KEYS:
        desired = str(routing.get(role) or "").strip()
        endpoints[role] = pick_endpoint(role, desired, hb_ports)

    n8n_primary = f"http://{T440}:5678"
    n8n_bridge = "http://127.0.0.1:5678"
    n8n_url = n8n_primary if health_ok(f"{n8n_primary}/healthz") else (
        n8n_bridge if health_ok(f"{n8n_bridge}/healthz") else n8n_primary
    )

    payload: dict[str, Any] = {
        "version": 1,
        "updated_at": int(time.time()),
        "updated_iso": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "endpoints": endpoints,
        "routing_state_path": str(ROUTING),
        "heartbeat_path": str(HEARTBEAT),
        "n8n_url": n8n_url,
        "n8n_primary": n8n_primary,
        "n8n_bridge": n8n_bridge,
        "mcp_worker": "http://127.0.0.1:8090",
        "forge_url": os.environ.get("FORGE_URL", "http://127.0.0.1:9100"),
        "zaya_llama_bin": os.environ.get(
            "ZAYA_LLAMA_BIN",
            "/home/cesarops/src/llama.cpp-zaya/build-vk/bin/llama-server",
        ),
        "slots": store.get("slots", {}),
    }

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    OUT_FILE.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    log(f"wrote {OUT_FILE}")

    if PATCH_ROUTING and ROUTING.is_file():
        patched = dict(routing)
        changed = False
        for role in ROLE_KEYS:
            ep = endpoints.get(role, {})
            url = ep.get("url")
            if url and patched.get(role) != url and ep.get("healthy"):
                patched[role] = url
                changed = True
        if changed:
            ROUTING.write_text(json.dumps(patched, indent=2) + "\n", encoding="utf-8")
            log(f"patched {ROUTING} from health poll")

    fail = sum(1 for r in ROLE_KEYS if not endpoints.get(r, {}).get("healthy"))
    log(f"roles healthy={len(ROLE_KEYS)-fail}/{len(ROLE_KEYS)} n8n={n8n_url}")
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
