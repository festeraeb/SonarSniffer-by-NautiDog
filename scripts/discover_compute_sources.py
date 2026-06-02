#!/usr/bin/env python3
"""Discover healthy compute endpoints from Forge, NautiInferer, and static pools.

Outputs normalized JSON so watchdog scripts can route missions to any available compute.
"""

from __future__ import annotations

import argparse
import json
import time
import urllib.error
import urllib.request
from typing import Any, Dict, List


def http_get_json(url: str, timeout: int = 4) -> Dict[str, Any]:
    req = urllib.request.Request(url, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
            return json.loads(raw)
    except Exception:
        return {}


def probe_endpoint(base_url: str, timeout: int = 3) -> bool:
    candidates = [
        base_url.rstrip("/") + "/v1/models",
        base_url.rstrip("/") + "/health",
    ]
    for u in candidates:
        req = urllib.request.Request(u, method="GET")
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                if 200 <= int(resp.status) < 300:
                    return True
        except Exception:
            continue
    return False


def add_node(nodes: List[Dict[str, Any]], seen: set, url: str, role: str, source: str, meta: Dict[str, Any] | None = None) -> None:
    base = url.rstrip("/")
    if not base or base in seen:
        return
    seen.add(base)
    entry = {
        "url": base,
        "role": (role or "general").lower(),
        "source": source,
        "healthy": probe_endpoint(base),
    }
    if meta:
        entry["meta"] = meta
    nodes.append(entry)


def parse_forge_nodes(doc: Dict[str, Any], nodes: List[Dict[str, Any]], seen: set) -> None:
    arr = doc.get("nodes") if isinstance(doc, dict) else None
    if not isinstance(arr, list):
        return
    for n in arr:
        if not isinstance(n, dict):
            continue
        role = str(n.get("role") or "general")
        ep = str(n.get("endpoint") or "")
        ip = str(n.get("node_ip") or n.get("ip") or "")
        port = n.get("port")
        if ep:
            add_node(nodes, seen, ep, role, "forge.cluster_nodes", {"name": n.get("name"), "gpu": n.get("gpu")})
        elif ip and isinstance(port, int):
            add_node(nodes, seen, f"http://{ip}:{port}", role, "forge.cluster_nodes", {"name": n.get("name"), "gpu": n.get("gpu")})


def parse_nauti_nodes(doc: Dict[str, Any], nodes: List[Dict[str, Any]], seen: set) -> None:
    arr = doc.get("nodes") if isinstance(doc, dict) else None
    if not isinstance(arr, list):
        return
    for n in arr:
        if not isinstance(n, dict):
            continue
        url = str(n.get("inference_url") or "")
        role = str(n.get("role") or "general")
        add_node(
            nodes,
            seen,
            url,
            role,
            "nauti.v1_nodes",
            {
                "id": n.get("id"),
                "gpu_name": n.get("gpu_name"),
                "online": n.get("online"),
                "vram_mb": n.get("vram_mb"),
            },
        )


def parse_extra_endpoints(extra: str, nodes: List[Dict[str, Any]], seen: set) -> None:
    for item in (extra or "").split(","):
        item = item.strip()
        if not item:
            continue
        # Supports role=url and plain url.
        if "=" in item:
            role, url = item.split("=", 1)
            add_node(nodes, seen, url.strip(), role.strip(), "env.extra")
        else:
            add_node(nodes, seen, item, "general", "env.extra")


def build_pools(nodes: List[Dict[str, Any]]) -> Dict[str, List[str]]:
    healthy = [n for n in nodes if n.get("healthy")]

    def urls_for_roles(*roles: str) -> List[str]:
        out: List[str] = []
        for wanted in roles:
            for n in healthy:
                role = str(n.get("role") or "general")
                if role == wanted and n["url"] not in out:
                    out.append(n["url"])
        return out

    thinker = urls_for_roles("thinker", "analysis", "reviewer", "general", "coder")
    reviewer = urls_for_roles("reviewer", "corrector", "thinker", "general", "coder")
    coder = urls_for_roles("coder", "coding", "general", "thinker", "reviewer")

    # Final fallback: any healthy node in discovery order.
    for n in healthy:
        u = n["url"]
        if u not in thinker:
            thinker.append(u)
        if u not in reviewer:
            reviewer.append(u)
        if u not in coder:
            coder.append(u)

    return {
        "thinker": thinker,
        "reviewer": reviewer,
        "corrector": reviewer,
        "coder": coder,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Discover compute endpoints for mission routing")
    ap.add_argument("--forge-url", default="http://127.0.0.1:9100")
    ap.add_argument("--nauti-url", default="http://127.0.0.1:8099")
    ap.add_argument("--extra-endpoints", default="")
    args = ap.parse_args()

    nodes: List[Dict[str, Any]] = []
    seen: set = set()

    # Bootstrap static known pools so discovery works even when APIs are down.
    static_defaults = [
        ("http://127.0.0.1:5001", "coder", "static.default"),
        ("http://127.0.0.1:5002", "reviewer", "static.default"),
        ("http://10.0.0.201:5200", "thinker", "static.default"),
        ("http://10.0.0.201:5201", "corrector", "static.default"),
        ("http://10.0.0.201:5202", "reviewer", "static.default"),
    ]
    for url, role, source in static_defaults:
        add_node(nodes, seen, url, role, source)

    parse_extra_endpoints(args.extra_endpoints, nodes, seen)

    forge_nodes = http_get_json(args.forge_url.rstrip("/") + "/cluster/nodes")
    parse_forge_nodes(forge_nodes, nodes, seen)

    nauti_nodes = http_get_json(args.nauti_url.rstrip("/") + "/v1/nodes")
    parse_nauti_nodes(nauti_nodes, nodes, seen)

    pools = build_pools(nodes)
    healthy = [n for n in nodes if n.get("healthy")]

    out = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "forge_url": args.forge_url,
        "nauti_url": args.nauti_url,
        "total_nodes": len(nodes),
        "healthy_nodes": len(healthy),
        "nodes": nodes,
        "pools": pools,
    }
    print(json.dumps(out, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
