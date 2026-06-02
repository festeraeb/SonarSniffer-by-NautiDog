#!/usr/bin/env python3
"""
GPU slot heartbeat store + dynamic llama-server restore (no fixed model preset).

Healthy slots are snapshotted from live /proc cmdline + /v1/models.
Down slots are restored from the last snapshot for that port (keyed by host:port and gpu_uuid).
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import time
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

import requests

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
HEARTBEAT_PATH = Path(
    os.environ.get(
        "GPU_SLOT_HEARTBEAT_PATH",
        os.environ.get(
            "GPU_SLOT_HEARTBEAT",
            "/data/cesarops/logs/gpu-slot-heartbeat.json",
        ),
    )
)
HEARTBEAT_FALLBACK = REPO / "var/gpu-slot-heartbeats/latest.json"
LLAMA = os.environ.get("LLAMA", "/home/cesarops/src/llama.cpp/build/bin/llama-server")
if not Path(LLAMA).is_file():
    alt = Path("/home/cesarops/llama.cpp/build/bin/llama-server")
    if alt.is_file():
        LLAMA = str(alt)
ZAYA_LLAMA = os.environ.get(
    "ZAYA_LLAMA_BIN",
    "/home/cesarops/src/llama.cpp-zaya/build-vk/bin/llama-server",
)


def pick_llama_binary(model_path: str, port: int | None = None) -> str:
    name = Path(model_path).name.lower()
    if "zaya" in name and Path(ZAYA_LLAMA).is_file():
        return ZAYA_LLAMA
    if port == 5203 and Path(ZAYA_LLAMA).is_file():
        return ZAYA_LLAMA
    return LLAMA
FORGE_URL = os.environ.get("FORGE_URL", "http://127.0.0.1:9100").rstrip("/")
FORGE_ROUTING_STATE = Path(
    os.environ.get(
        "FORGE_ROUTING_STATE",
        REPO / "cesarops-forge-v2/routing_state.json",
    )
)
LAN_HOST = os.environ.get("CESAROPS2_LAN_HOST", "10.0.0.201")


def log(msg: str) -> None:
    print(f"[gpu-slot] {msg}", flush=True)


def hostname_node() -> str:
    try:
        h = Path("/etc/hostname").read_text().strip().lower()
    except OSError:
        h = "local"
    if "t440" in h:
        return "t440"
    return "cesarops2"


def load_store() -> dict[str, Any]:
    for path in (HEARTBEAT_PATH, HEARTBEAT_FALLBACK):
        if path.is_file():
            try:
                return json.loads(path.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, OSError):
                pass
    return {"version": 1, "slots": {}, "by_gpu_uuid": {}}


def save_store(data: dict[str, Any]) -> None:
    data["updated_at"] = int(time.time())
    for path in (HEARTBEAT_PATH, HEARTBEAT_FALLBACK):
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(data, indent=2), encoding="utf-8")
        except OSError as e:
            log(f"warn: cannot write {path}: {e}")


def slot_key(host: str, port: int) -> str:
    return f"{host}:{port}"


def probe_health(host: str, port: int) -> tuple[bool, str]:
    base = f"http://{host}:{port}"
    try:
        r = requests.get(f"{base}/v1/models", timeout=5)
        if r.ok:
            data = r.json()
            mid = (
                data.get("data", [{}])[0].get("id")
                if isinstance(data.get("data"), list)
                else ""
            )
            return True, str(mid or "")
    except requests.RequestException:
        pass
    try:
        r = requests.get(f"{base}/health", timeout=4)
        if r.ok:
            return True, ""
    except requests.RequestException:
        pass
    return False, ""


def pid_on_port(port: int) -> int | None:
    try:
        out = subprocess.check_output(
            ["ss", "-ltnp"],
            text=True,
            timeout=8,
            stderr=subprocess.DEVNULL,
        )
    except (subprocess.SubprocessError, FileNotFoundError):
        return None
    needle = f":{port} "
    for line in out.splitlines():
        if needle not in line:
            continue
        m = re.search(r"pid=(\d+)", line)
        if m:
            return int(m.group(1))
    return None


def read_cmdline(pid: int) -> list[str]:
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes()
    except OSError:
        return []
    parts = [p.decode("utf-8", errors="replace") for p in raw.split(b"\0") if p]
    return parts


def gpu_uuid_for_pid(pid: int) -> tuple[str, str]:
    """Return (gpu_uuid, gpu_name) for compute pid."""
    try:
        out = subprocess.check_output(
            [
                "nvidia-smi",
                "--query-compute-apps=gpu_uuid,pid",
                "--format=csv,noheader",
            ],
            text=True,
            timeout=8,
            stderr=subprocess.DEVNULL,
        )
    except (subprocess.SubprocessError, FileNotFoundError):
        return "", ""
    uuid = ""
    for line in out.splitlines():
        parts = [p.strip() for p in line.split(",")]
        if len(parts) >= 2 and parts[1] == str(pid):
            uuid = parts[0]
            break
    name = ""
    if uuid:
        try:
            out2 = subprocess.check_output(
                [
                    "nvidia-smi",
                    "--query-gpu=gpu_uuid,name",
                    "--format=csv,noheader",
                ],
                text=True,
                timeout=8,
                stderr=subprocess.DEVNULL,
            )
            for line in out2.splitlines():
                u, n = [p.strip() for p in line.split(",", 1)]
                if u == uuid:
                    name = n
                    break
        except subprocess.SubprocessError:
            pass
    return uuid, name


def parse_llama_cmdline(argv: list[str]) -> dict[str, Any]:
    """Extract model_path and launch metadata from a running llama-server argv."""
    rec: dict[str, Any] = {
        "model_path": "",
        "llama_dev": "",
        "reasoning": "",
        "ctx": 0,
        "cuda_visible": "",
        "launch_argv": argv,
    }
    if not argv:
        return rec
    i = 0
    while i < len(argv):
        tok = argv[i]
        if tok in ("-m", "--model") and i + 1 < len(argv):
            rec["model_path"] = argv[i + 1]
            i += 2
            continue
        if tok.startswith("-m") and len(tok) > 2:
            rec["model_path"] = tok[2:]
            i += 1
            continue
        if tok in ("-dev", "--device") and i + 1 < len(argv):
            rec["llama_dev"] = argv[i + 1]
            i += 2
            continue
        if tok == "--reasoning" and i + 1 < len(argv):
            rec["reasoning"] = argv[i + 1]
            i += 2
            continue
        if tok in ("-c", "--ctx-size") and i + 1 < len(argv):
            try:
                rec["ctx"] = int(argv[i + 1])
            except ValueError:
                pass
            i += 2
            continue
        i += 1
    # P100 scripts use CUDA_VISIBLE_DEVICES in parent; not in argv — detect via nvidia-smi
    return rec


def snapshot_port(host: str, port: int) -> dict[str, Any] | None:
    ok, model_id = probe_health(host, port)
    if not ok:
        return None
    pid = pid_on_port(port) if host in ("127.0.0.1", "localhost") else None
    if pid is None and host not in ("127.0.0.1", "localhost"):
        # remote host: store minimal snapshot from HTTP only
        return {
            "host": host,
            "port": port,
            "model_path": "",
            "model_id": model_id,
            "gpu_uuid": "",
            "gpu_name": "",
            "timestamp": int(time.time()),
            "healthy": True,
            "remote": True,
        }
    if pid is None:
        return {
            "host": host,
            "port": port,
            "model_path": "",
            "model_id": model_id,
            "gpu_uuid": "",
            "gpu_name": "",
            "timestamp": int(time.time()),
            "healthy": True,
        }
    argv = read_cmdline(pid)
    meta = parse_llama_cmdline(argv)
    uuid, gname = gpu_uuid_for_pid(pid)
    return {
        "host": host,
        "port": port,
        "model_path": meta.get("model_path") or "",
        "model_id": model_id,
        "gpu_uuid": uuid,
        "gpu_name": gname,
        "llama_dev": meta.get("llama_dev") or "",
        "reasoning": meta.get("reasoning") or "",
        "ctx": meta.get("ctx") or 0,
        "launch_argv": argv,
        "timestamp": int(time.time()),
        "healthy": True,
    }


def stop_port(port: int) -> None:
    for pat in (
        f"llama-server.*--port {port}",
        f"llama-server.*-port {port}",
        f"koboldcpp.*--port {port}",
    ):
        subprocess.run(["pkill", "-f", pat], capture_output=True)
    subprocess.run(
        ["fuser", "-k", f"{port}/tcp"],
        capture_output=True,
    )
    time.sleep(2)


def launch_from_snapshot(hb: dict[str, Any]) -> bool:
    port = int(hb["port"])
    model = (hb.get("model_path") or "").strip()
    argv = hb.get("launch_argv")
    if isinstance(argv, list) and len(argv) >= 2 and Path(argv[0]).name in (
        "llama-server",
        "koboldcpp",
    ):
        stop_port(port)
        if model and "zaya" in model.lower() and Path(ZAYA_LLAMA).is_file():
            argv = list(argv)
            argv[0] = ZAYA_LLAMA
        # Re-exec stored argv (same port/model flags as last healthy run)
        env = os.environ.copy()
        log_path = Path(f"/tmp/cesarops-gpu-slot-{port}.log")
        log_path.parent.mkdir(parents=True, exist_ok=True)
        with open(log_path, "ab") as lf:
            subprocess.Popen(
                argv,
                stdout=lf,
                stderr=lf,
                stdin=subprocess.DEVNULL,
                start_new_session=True,
                env=env,
            )
        log(f"restored port {port} via saved argv ({len(argv)} args)")
        return True

    if not model or not Path(model).is_file():
        log(f"cannot restore :{port} — no model_path in heartbeat")
        return False

    stop_port(port)
    llama_dev = hb.get("llama_dev") or "CUDA0"
    ctx = int(hb.get("ctx") or 4096)
    reasoning = hb.get("reasoning") or "off"
    cmd = [
        pick_llama_binary(model, port),
        "-m",
        model,
        "--host",
        "0.0.0.0",
        "--port",
        str(port),
        "-dev",
        llama_dev,
        "-ngl",
        "99",
        "-fa",
        "auto",
        "-ctk",
        "q8_0",
        "-ctv",
        "q8_0",
        "-ub",
        "384",
        "-c",
        str(ctx),
        "-t",
        "4",
        "-np",
        "1",
        "--reasoning",
        str(reasoning),
        "--timeout",
        "600",
    ]
    log_path = Path(f"/tmp/cesarops-gpu-slot-{port}.log")
    with open(log_path, "ab") as lf:
        subprocess.Popen(
            cmd,
            stdout=lf,
            stderr=lf,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
        )
    log(f"restored :{port} model={Path(model).name} dev={llama_dev}")
    return True


def ports_from_forge_routing() -> list[tuple[str, int]]:
    out: list[tuple[str, int]] = []
    if not FORGE_ROUTING_STATE.is_file():
        return out
    try:
        data = json.loads(FORGE_ROUTING_STATE.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return out
    local_hosts = {"127.0.0.1", "localhost", LAN_HOST.lower()}
    for key in (
        "thinker_endpoint",
        "coder_endpoint",
        "draft_endpoint",
        "corrector_endpoint",
        "reviewer_endpoint",
    ):
        url = str(data.get(key) or "").strip()
        if not url:
            continue
        p = urlparse(url)
        host = (p.hostname or "").lower()
        port = p.port
        if not port:
            continue
        if host in local_hosts:
            out.append((host if host not in ("localhost",) else "127.0.0.1", port))
    return out


def ports_from_env() -> list[tuple[str, int]]:
    raw = os.environ.get("GPU_WATCH_PORTS", os.environ.get("C2_WATCH_PORTS", ""))
    out: list[tuple[str, int]] = []
    for part in raw.replace(",", " ").split():
        part = part.strip()
        if not part:
            continue
        if ":" in part:
            h, p = part.rsplit(":", 1)
            if p.isdigit():
                out.append((h, int(p)))
        elif part.isdigit():
            out.append(("127.0.0.1", int(part)))
    return out


def discover_local_llama_ports() -> list[tuple[str, int]]:
    out: list[tuple[str, int]] = []
    try:
        text = subprocess.check_output(["ss", "-ltnp"], text=True, timeout=8)
    except (subprocess.SubprocessError, FileNotFoundError):
        return out
    for line in text.splitlines():
        if "llama-server" not in line and "koboldcpp" not in line:
            continue
        m = re.search(r":(\d+)\s", line)
        if m:
            out.append(("127.0.0.1", int(m.group(1))))
    return out


def merge_forge_gpus(store: dict[str, Any]) -> None:
    """Enrich snapshots from Forge /cluster/gpus when reachable."""
    try:
        r = requests.get(f"{FORGE_URL}/cluster/gpus", timeout=8)
        if not r.ok:
            return
        payload = r.json()
    except requests.RequestException:
        return
    gpus = payload.get("gpus") or payload.get("cards") or []
    if not isinstance(gpus, list):
        return
    slots = store.setdefault("slots", {})
    for g in gpus:
        if not isinstance(g, dict):
            continue
        port = int(g.get("port") or g.get("port_hint") or 0)
        host = str(g.get("host") or "127.0.0.1")
        if not port:
            continue
        if host not in ("127.0.0.1", LAN_HOST, "10.0.0.61", "local"):
            continue
        key = slot_key(host if host != "local" else "127.0.0.1", port)
        cmd_model = (g.get("cmdline_model") or "").strip()
        uuid = (g.get("gpu_uuid") or "").strip()
        if not cmd_model and not uuid:
            continue
        existing = slots.get(key) or {}
        if cmd_model and not existing.get("model_path"):
            existing["model_path"] = cmd_model
        if uuid:
            existing["gpu_uuid"] = uuid
        existing["port"] = port
        existing["host"] = host if host != "local" else "127.0.0.1"
        existing["timestamp"] = int(time.time())
        slots[key] = existing
        if uuid:
            store.setdefault("by_gpu_uuid", {})[uuid] = key


def watch_list() -> list[tuple[str, int]]:
    seen: set[tuple[str, int]] = set()
    merged: list[tuple[str, int]] = []
    for src in (
        ports_from_env,
        ports_from_forge_routing,
        discover_local_llama_ports,
    ):
        for item in src():
            if item not in seen:
                seen.add(item)
                merged.append(item)
    # Sensible defaults if nothing configured
    if not merged:
        node = hostname_node()
        if node == "t440":
            merged = [("127.0.0.1", 5001), ("127.0.0.1", 5002)]
        else:
            merged = [("127.0.0.1", 5200), ("127.0.0.1", 5201), ("127.0.0.1", 5202)]
    return merged


def tick(record: bool = True, recover: bool = True) -> dict[str, Any]:
    store = load_store()
    merge_forge_gpus(store)
    ports = watch_list()
    results: dict[str, Any] = {"ports": [], "recorded": 0, "recovered": 0}

    for host, port in ports:
        key = slot_key(host, port)
        ok, _mid = probe_health(host, port)
        entry: dict[str, Any] = {"host": host, "port": port, "healthy": ok}

        if ok and record:
            snap = snapshot_port(host, port)
            if snap:
                store["slots"][key] = snap
                if snap.get("gpu_uuid"):
                    store.setdefault("by_gpu_uuid", {})[snap["gpu_uuid"]] = key
                results["recorded"] += 1
                entry["action"] = "snapshot"
            else:
                entry["action"] = "healthy_no_snapshot"
        elif not ok and recover:
            hb = store.get("slots", {}).get(key)
            if not hb and store.get("by_gpu_uuid"):
                # try uuid-indexed slot on same port only
                pass
            if hb and (hb.get("model_path") or hb.get("launch_argv")):
                entry["action"] = "recover"
                if launch_from_snapshot(hb):
                    results["recovered"] += 1
                    entry["recover"] = "started"
                else:
                    entry["recover"] = "failed"
            else:
                entry["action"] = "down_no_heartbeat"
        else:
            entry["action"] = "skip"

        results["ports"].append(entry)

    save_store(store)
    return results


def main() -> None:
    import argparse

    ap = argparse.ArgumentParser(description="GPU slot heartbeat tick")
    ap.add_argument("command", choices=["tick", "show", "recover-port"], nargs="?", default="tick")
    ap.add_argument("--port", type=int, default=0)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--no-recover", action="store_true")
    ap.add_argument("--no-record", action="store_true")
    args = ap.parse_args()

    if args.command == "show":
        print(json.dumps(load_store(), indent=2))
        return
    if args.command == "recover-port" and args.port:
        store = load_store()
        key = slot_key(args.host, args.port)
        hb = store.get("slots", {}).get(key)
        if not hb:
            raise SystemExit(f"no heartbeat for {key}")
        launch_from_snapshot(hb)
        return

    res = tick(record=not args.no_record, recover=not args.no_recover)
    log(json.dumps(res))


if __name__ == "__main__":
    main()
