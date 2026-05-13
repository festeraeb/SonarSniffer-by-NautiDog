#!/usr/bin/env python3
"""
CESAROPS Remote Task Dispatcher — Multi-node capability-based dispatch

Cluster role architecture:
  - Pi  (10.0.0.226)  CONDUCTOR  — orchestrates, downloads, slices, dispatches
  - i7  (10.0.0.56)   GPU/TPU    — NVIDIA P1000 + Coral TPU + CUDA 12.6 (primary heavy lifting)
  - Laptop (10.0.0.69) OVERFLOW  — fills in when not busy
  - Xeon (10.0.0.55)  OFFLINE    — out of commission; hardware (TPU/P1000) moved to i7

Uses paramiko for SSH. Credentials from .env or environment variables.
"""

import json
import os
import time
from pathlib import Path
from typing import Optional
from datetime import datetime, timezone

try:
    import paramiko
    HAS_PARAMIKO = True
except ImportError:
    HAS_PARAMIKO = False


# ── Config ───────────────────────────────────────────────────────────────────

def _load_env(path: Path) -> dict:
    env = {}
    if path.exists():
        for line in path.read_text(encoding='utf-8').splitlines():
            line = line.strip()
            if line and not line.startswith('#') and '=' in line:
                k, _, v = line.partition('=')
                env[k.strip()] = v.strip()
    return env

_dotenv = _load_env(Path(__file__).parent / ".env")

# Pi (Janitor)
PI_HOST      = os.environ.get("PI_HOST",      _dotenv.get("PI_HOST",      "10.0.0.226"))
PI_TAILSCALE = os.environ.get("PI_TAILSCALE", _dotenv.get("PI_TAILSCALE", "100.127.66.32"))  # reliable jump host
PI_USER = os.environ.get("PI_USER", _dotenv.get("PI_USER", "pi"))
PI_PASS = os.environ.get("PI_PASS", _dotenv.get("PI_PASS", ""))
PI_KEY  = os.environ.get("PI_KEY",  _dotenv.get("PI_KEY",  ""))
PI_WORK = os.environ.get("PI_WORK", _dotenv.get("PI_WORK", "/home/pi/cesarops/sync"))

# Xenon (CPU server — offline when hardware fault; restore when back online)
XENON_HOST = os.environ.get("XENON_HOST", _dotenv.get("XENON_HOST", "10.0.0.56"))  # i7 (Xeon offline, TPU+P1000 moved here)
XENON_USER = os.environ.get("XENON_USER", _dotenv.get("XENON_USER", "cesarops"))
XENON_PASS = os.environ.get("XENON_PASS", _dotenv.get("XENON_PASS", ""))
XENON_KEY = os.environ.get("XENON_KEY", _dotenv.get("XENON_KEY", ""))
XENON_WORK = os.environ.get("XENON_WORK", _dotenv.get("XENON_WORK", "/home/cesarops/cesarops/sync"))

# i7 (GPU/TPU workhorse — NVIDIA P1000, Coral TPU, CUDA 12.6, tpu-server:5001)
I7_HOST      = os.environ.get("I7_HOST",      _dotenv.get("I7_HOST",      "10.0.0.56"))
I7_USER      = os.environ.get("I7_USER",      _dotenv.get("I7_USER",      "cesarops"))
I7_PASS      = os.environ.get("I7_PASS",      _dotenv.get("I7_PASS",      ""))
I7_KEY       = os.environ.get("I7_KEY",       _dotenv.get("I7_KEY",       ""))
I7_WORK      = os.environ.get("I7_WORK",      _dotenv.get("I7_WORK",      "/home/cesarops"))
I7_TPU_PORT  = int(os.environ.get("I7_TPU_PORT", _dotenv.get("I7_TPU_PORT", "5001")))
# i7 is only reachable via Pi jump (Tailscale ACL blocks direct TCP to i7)
I7_JUMP_HOST = os.environ.get("I7_JUMP_HOST", _dotenv.get("I7_JUMP_HOST", PI_TAILSCALE))
I7_JUMP_USER = os.environ.get("I7_JUMP_USER", _dotenv.get("I7_JUMP_USER", PI_USER))

# T440 (new execution / overflow worker — dual Tesla P100)
T440_HOST     = os.environ.get("T440_HOST",     _dotenv.get("T440_HOST",     "10.0.0.61"))
T440_USER     = os.environ.get("T440_USER",     _dotenv.get("T440_USER",     "executor"))
T440_PASS     = os.environ.get("T440_PASS",     _dotenv.get("T440_PASS",     ""))
T440_KEY      = os.environ.get("T440_KEY",      _dotenv.get("T440_KEY",      ""))
T440_WORK     = os.environ.get("T440_WORK",     _dotenv.get("T440_WORK",     "/home/executor/wreckhunter2000-1"))
T440_TAILSCALE = os.environ.get("T440_TAILSCALE", _dotenv.get("T440_TAILSCALE", ""))

RESEARCH_MODE = os.environ.get("RESEARCH_MODE", _dotenv.get("RESEARCH_MODE", "false")).strip().lower() in ("1", "true", "yes", "on")
CODING_AGENT_MODE = os.environ.get("CODING_AGENT_MODE", _dotenv.get("CODING_AGENT_MODE", "false")).strip().lower() in ("1", "true", "yes", "on")

# Laptop (overflow / fill-in — Windows host)
LAPTOP_HOST = os.environ.get("LAPTOP_HOST", _dotenv.get("LAPTOP_HOST", "10.0.0.69"))
LAPTOP_USER = os.environ.get("LAPTOP_USER", _dotenv.get("LAPTOP_USER", "thomf"))
LAPTOP_PASS = os.environ.get("LAPTOP_PASS", _dotenv.get("LAPTOP_PASS", ""))
LAPTOP_KEY  = os.environ.get("LAPTOP_KEY",  _dotenv.get("LAPTOP_KEY",  ""))
LAPTOP_WORK = os.environ.get("LAPTOP_WORK", _dotenv.get("LAPTOP_WORK",
    "C:/Users/thomf/programming/wreckhunter2000-1"))

# ── Capability roles ─────────────────────────────────────────────────────────

ROLE_CONDUCTOR = "conductor"   # Pi     — orchestrates, downloads, slices, dispatches
ROLE_GPU_TPU   = "gpu_tpu"     # i7     — heavy GPU + TPU inference (primary)
ROLE_OVERFLOW  = "overflow"    # Laptop — fills in when not busy
ROLE_CPU       = "cpu"         # Xeon / other-i7 — CPU-only, lighter tasks

# Task type → capability role
TASK_ROLE_MAP = {
    "tpu":    ROLE_GPU_TPU,
    "gpu":    ROLE_GPU_TPU,
    "hybrid": ROLE_GPU_TPU,
    "cpu":    ROLE_CPU,
    "slice":  ROLE_CONDUCTOR,
}


class SSHNode:
    """Represents a remote node with SSH access. Supports optional jump host."""

    def __init__(self, host: str, user: str, password: str = "", key_path: str = "",
                 jump_host: str = "", jump_user: str = "", jump_password: str = "",
                 jump_key: str = ""):
        self.host = host
        self.user = user
        self.password = password
        self.key_path = key_path
        self.jump_host = jump_host
        self.jump_user = jump_user
        self.jump_password = jump_password
        self.jump_key = jump_key
        self._client: Optional[paramiko.SSHClient] = None
        self._jump_client: Optional[paramiko.SSHClient] = None

    def _make_client(self) -> paramiko.SSHClient:
        c = paramiko.SSHClient()
        c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        return c

    def connect(self) -> bool:
        """Open SSH connection, tunnelling through jump_host if configured."""
        if not HAS_PARAMIKO:
            raise RuntimeError("paramiko not installed — pip install paramiko")

        sock = None
        if self.jump_host:
            # Open jump connection first
            self._jump_client = self._make_client()
            jkey = self.jump_key or (self.key_path if not self.jump_password else "")
            if jkey and Path(jkey).exists():
                self._jump_client.connect(self.jump_host, username=self.jump_user,
                                          key_filename=jkey, timeout=10)
            elif self.jump_password:
                self._jump_client.connect(self.jump_host, username=self.jump_user,
                                          password=self.jump_password, timeout=10)
            else:
                raise RuntimeError(f"No auth method for jump {self.jump_user}@{self.jump_host}")
            transport = self._jump_client.get_transport()
            sock = transport.open_channel(
                "direct-tcpip", (self.host, 22), ("127.0.0.1", 0), timeout=10
            )

        self._client = self._make_client()
        if self.key_path and Path(self.key_path).exists():
            self._client.connect(
                self.host, username=self.user,
                key_filename=self.key_path, sock=sock, timeout=10,
            )
        elif self.password:
            self._client.connect(
                self.host, username=self.user,
                password=self.password, sock=sock, timeout=10,
            )
        else:
            raise RuntimeError(f"No auth method for {self.user}@{self.host}")
        return True

    def close(self):
        if self._client:
            self._client.close()
            self._client = None

    def run(self, cmd: str, timeout: int = 3600) -> dict:
        """Run command, return {stdout, stderr, exit_code, duration_s}."""
        if not self._client:
            self.connect()

        start = time.time()
        stdin, stdout, stderr = self._client.exec_command(cmd, timeout=timeout)
        exit_code = stdout.channel.recv_exit_status()
        duration = time.time() - start

        return {
            "stdout": stdout.read().decode("utf-8", errors="replace"),
            "stderr": stderr.read().decode("utf-8", errors="replace"),
            "exit_code": exit_code,
            "duration_s": round(duration, 2),
        }

    def ping(self) -> bool:
        """Quick connectivity check."""
        try:
            if not self._client:
                self.connect()
            result = self.run("echo pong", timeout=5)
            return result["exit_code"] == 0 and "pong" in result["stdout"]
        except Exception:
            return False


# ── Task Definitions ─────────────────────────────────────────────────────────

def build_pi_slice_task(
    area_name: str,
    bbox: list,
    sources: list,
    tile_size: int = 1024,
    target_resolution: float = 10.0,
    mission_json: str = "",
) -> str:
    """Build the shell command for Pi to run: VRT stack → slice → route."""
    sources_str = " ".join(sources)
    cmd = (
        f"cd {PI_WORK} && "
        f"echo '[PI] Starting slice pipeline for {area_name}' && "
        f"mkdir -p tiles/cpu tiles/tpu tiles/gpu tiles/hybrid && "
        f"./slicer vrt "
        f"{sources_str} "
        f"--output tiles "
        f"--tile-size {tile_size} "
        f"--target-resolution {target_resolution}"
    )
    if mission_json:
        cmd += f" --mission {mission_json}"
    cmd += (
        f" && echo '[PI] Slicing complete — tiles staged in delegate folders' "
        f"&& ls -la tiles/*/ | tail -20"
    )
    return cmd


def build_xenon_process_task(
    tiles_dir: str = "",
    delegate: str = "",
) -> str:
    """Build the shell command for Xenon to process staged tiles."""
    work = XENON_WORK
    cmd = (
        f"cd {work} && "
        f"echo '[XENON] Starting tile processing'"
    )

    if delegate:
        # Only process specific delegate folder
        cmd += (
            f" && echo '[XENON] Processing {delegate} tiles...' "
            f"&& python cesarops_engine.py --tiles-dir tiles/{delegate} --delegate {delegate}"
        )
    else:
        # Process all delegate folders in order: TPU first (fastest), then GPU, then CPU
        cmd += (
            f" && for delegate in tpu gpu cpu hybrid; do "
            f"  count=$(ls tiles/$delegate/*.bin 2>/dev/null | wc -l); "
            f"  if [ $count -gt 0 ]; then "
            f"    echo '[XENON] Processing $delegate: $count tiles'; "
            f"    python cesarops_engine.py --tiles-dir tiles/$delegate --delegate $delegate; "
            f"  fi; "
            f"done"
        )

    cmd += f" && echo '[XENON] Processing complete'"
    return cmd


def build_xenon_tpu_health() -> str:
    """Check TPU server health on Xenon."""
    return (
        f"cd {XENON_WORK} && "
        f"curl -s http://localhost:5001/health 2>/dev/null || echo '{{\"status\": \"unreachable\"}}'"
    )


def build_xenon_tpu_start() -> str:
    """Start TPU server on Xenon in background, searching likely locations for tpu_server.py."""
    # Search order: repo sync dir, common install paths
    search_paths = [
        f"{XENON_WORK}",
        f"{XENON_WORK}/../cesarops-core",
        "/home/cesarops/cesarops-core",
        "/home/cesarops/cesarops/cesarops-core",
        "/opt/cesarops",
    ]
    find_cmd = " || ".join(
        f"[ -f {p}/tpu_server.py ] && echo {p}" for p in search_paths
    )
    return (
        f"TPU_DIR=$( {find_cmd} | head -1 ) && "
        f"if [ -z \"$TPU_DIR\" ]; then "
        f"  echo '{{\"status\": \"tpu_server_not_found\"}}'; exit 0; "
        f"fi && "
        f"if ! curl -s http://localhost:5001/health > /dev/null 2>&1; then "
        f"  cd $TPU_DIR && "
        f"  nohup python tpu_server.py --port 5001 > /tmp/tpu_server.log 2>&1 & "
        f"  sleep 5; "
        f"fi && "
        f"curl -s http://localhost:5001/health 2>/dev/null || echo '{{\"status\": \"start_failed\"}}'"
    )


def build_i7_process_task(delegate: str = "") -> str:
    """Build shell command for i7 to process GPU/TPU tiles."""
    work = I7_WORK
    cmd = (
        f"cd {work} && "
        f"echo '[I7] Starting GPU/TPU tile processing'"
    )
    if delegate:
        cmd += (
            f" && echo '[I7] Processing {delegate} tiles...' "
            f"&& python cesarops_engine.py --tiles-dir tiles/{delegate} --delegate {delegate}"
        )
    else:
        # GPU/TPU delegates first, then hybrid
        cmd += (
            f" && for delegate in tpu gpu hybrid; do "
            f"  count=$(ls tiles/$delegate/*.bin 2>/dev/null | wc -l); "
            f"  if [ $count -gt 0 ]; then "
            f"    echo '[I7] Processing $delegate: $count tiles'; "
            f"    python cesarops_engine.py --tiles-dir tiles/$delegate --delegate $delegate; "
            f"  fi; "
            f"done"
        )
    cmd += f" && echo '[I7] Processing complete'"
    return cmd


def build_i7_tpu_health() -> str:
    """Check TPU server health on i7."""
    return (
        f"curl -s http://localhost:{I7_TPU_PORT}/health 2>/dev/null "
        f"|| echo '{{\"status\": \"unreachable\"}}'"
    )


def build_i7_tpu_start() -> str:
    """Start TPU server on i7 if not already running."""
    search_paths = [
        f"{I7_WORK}",
        f"{I7_WORK}/wreckhunter2000-1",
        "/home/cesarops/wreckhunter2000-1",
        "/home/cesarops/cesarops-core",
    ]
    find_cmd = " || ".join(
        f"[ -f {p}/tpu_server.py ] && echo {p}" for p in search_paths
    )
    return (
        f"TPU_DIR=$( {find_cmd} | head -1 ) && "
        f"if [ -z \"$TPU_DIR\" ]; then "
        f"  echo '{{\"status\": \"tpu_server_not_found\"}}'; exit 0; "
        f"fi && "
        f"if ! curl -s http://localhost:{I7_TPU_PORT}/health > /dev/null 2>&1; then "
        f"  cd $TPU_DIR && "
        f"  nohup python tpu_server.py --port {I7_TPU_PORT} > /tmp/tpu_server.log 2>&1 & "
        f"  sleep 5; "
        f"fi && "
        f"curl -s http://localhost:{I7_TPU_PORT}/health 2>/dev/null "
        f"|| echo '{{\"status\": \"start_failed\"}}'"
    )


def build_data_inventory(lakes: list = None, data_root: str = None) -> str:
    """Build shell command to inventory downloaded data on Pi."""
    root = data_root or f"{PI_WORK}/downloads"
    lake_list = " ".join(lakes) if lakes else "michigan superior huron erie ontario"
    return (
        f"python3 -c \""
        f"import json, os; "
        f"root = '{root}'; "
        f"inv = {{}}; "
        f"for lake in '{lake_list}'.split(): "
        f"  d = os.path.join(root, lake); "
        f"  inv[lake] = {{'exists': os.path.isdir(d), 'files': len(os.listdir(d)) if os.path.isdir(d) else 0}}; "
        f"print(json.dumps(inv))"
        f"\""
    )


# ── High-level dispatcher ────────────────────────────────────────────────────

class TaskDispatcher:
    """Dispatches tasks to cluster nodes by capability role.

    Roles:
      pi     — CONDUCTOR: orchestrates, downloads, slices
      i7     — GPU/TPU:   NVIDIA P1000 + Coral TPU (primary heavy lifting)
      laptop — OVERFLOW:  fills in when not busy
      xenon  — CPU:       CPU-only tasks when back online
    """

    def __init__(self):
        self.pi     = SSHNode(PI_HOST,     PI_USER,     PI_PASS,     PI_KEY)     if PI_PASS     or PI_KEY     else None
        self.xenon  = SSHNode(XENON_HOST,  XENON_USER,  XENON_PASS,  XENON_KEY)  if XENON_PASS  or XENON_KEY  else None
        self.i7     = SSHNode(I7_HOST,     I7_USER,     I7_PASS,     I7_KEY,
                              jump_host=I7_JUMP_HOST, jump_user=I7_JUMP_USER,
                              jump_key=I7_KEY)                                   if I7_PASS     or I7_KEY     else None
        self.t440   = SSHNode(T440_HOST,   T440_USER,   T440_PASS,   T440_KEY)   if T440_PASS   or T440_KEY   else None
        self.laptop = SSHNode(LAPTOP_HOST, LAPTOP_USER, LAPTOP_PASS, LAPTOP_KEY) if LAPTOP_PASS or LAPTOP_KEY else None
        self.task_log = []
        # Dynamic node registry — load extra nodes from EXTRA_NODES env/json
        # Format: [{"name":"opti1","host":"100.x.x.x","user":"u","key":"/path"}]
        self.extra_nodes: dict[str, SSHNode] = {}
        self._load_extra_nodes()

    def _load_extra_nodes(self):
        """Load additional compute nodes from EXTRA_NODES env var or .env."""
        raw = os.environ.get("EXTRA_NODES", _dotenv.get("EXTRA_NODES", ""))
        if not raw:
            return
        try:
            nodes = json.loads(raw)
            for n in nodes:
                name = n["name"]
                self.extra_nodes[name] = SSHNode(
                    n["host"], n["user"],
                    n.get("password", ""), n.get("key", ""),
                )
        except (json.JSONDecodeError, KeyError) as e:
            print(f"⚠ Failed to parse EXTRA_NODES: {e}")

    def status(self) -> dict:
        """Ping all nodes, return connectivity status including TPU state and data inventory."""
        result = {"timestamp": datetime.now(timezone.utc).isoformat(), "nodes": {}}

        if self.pi:
            pi_online = self.pi.ping()
            result["nodes"]["pi"] = {
                "host": PI_HOST,
                "online": pi_online,
                "work_dir": PI_WORK,
            }
            # Data inventory on Pi
            if pi_online:
                try:
                    inv_result = self.pi.run(build_data_inventory(), timeout=15)
                    if inv_result["exit_code"] == 0 and inv_result["stdout"].strip():
                        result["nodes"]["pi"]["data_inventory"] = json.loads(inv_result["stdout"].strip())
                except Exception:
                    result["nodes"]["pi"]["data_inventory"] = {"error": "inventory_failed"}
        else:
            result["nodes"]["pi"] = {"host": PI_HOST, "online": False, "reason": "no credentials"}

        if self.xenon:
            xenon_online = self.xenon.ping()
            result["nodes"]["xenon"] = {
                "host": XENON_HOST,
                "online": xenon_online,
                "work_dir": XENON_WORK,
                "role": ROLE_CPU,
            }
            if xenon_online:
                # Check TPU — auto-start if unreachable
                try:
                    tpu_result = self.xenon.run(build_xenon_tpu_health(), timeout=10)
                    raw = tpu_result["stdout"].strip() if tpu_result["exit_code"] == 0 else ""
                    tpu_info = json.loads(raw) if raw else {"status": "error"}
                    if tpu_info.get("status") in ("unreachable", "error", "start_failed", None):
                        # Auto-start tpu_server.py
                        start_result = self.xenon.run(build_xenon_tpu_start(), timeout=20)
                        raw2 = start_result["stdout"].strip()
                        tpu_info = json.loads(raw2) if raw2 else {"status": "start_failed"}
                        tpu_info["auto_started"] = True
                    result["nodes"]["xenon"]["tpu"] = tpu_info
                except Exception as e:
                    result["nodes"]["xenon"]["tpu"] = {"status": "unreachable", "error": str(e)}
            else:
                result["nodes"]["xenon"]["tpu"] = {"status": "node_offline"}
        else:
            result["nodes"]["xenon"] = {"host": XENON_HOST, "online": False, "reason": "no credentials"}

        # i7 — GPU/TPU workhorse
        if self.i7:
            i7_online = self.i7.ping()
            result["nodes"]["i7"] = {
                "host": I7_HOST,
                "online": i7_online,
                "work_dir": I7_WORK,
                "role": ROLE_GPU_TPU,
            }
            if i7_online:
                try:
                    tpu_result = self.i7.run(build_i7_tpu_health(), timeout=10)
                    raw = tpu_result["stdout"].strip() if tpu_result["exit_code"] == 0 else ""
                    tpu_info = json.loads(raw) if raw else {"status": "error"}
                    if tpu_info.get("status") in ("unreachable", "error", "start_failed", None):
                        start_result = self.i7.run(build_i7_tpu_start(), timeout=20)
                        raw2 = start_result["stdout"].strip()
                        tpu_info = json.loads(raw2) if raw2 else {"status": "start_failed"}
                        tpu_info["auto_started"] = True
                    result["nodes"]["i7"]["tpu"] = tpu_info
                except Exception as e:
                    result["nodes"]["i7"]["tpu"] = {"status": "unreachable", "error": str(e)}
            else:
                result["nodes"]["i7"]["tpu"] = {"status": "node_offline"}
        else:
            result["nodes"]["i7"] = {"host": I7_HOST, "online": False, "reason": "no credentials"}

        if self.t440:
            t440_online = self.t440.ping()
            result["nodes"]["t440"] = {
                "host": T440_HOST,
                "online": t440_online,
                "work_dir": T440_WORK,
                "role": "t440",
            }
        else:
            result["nodes"]["t440"] = {"host": T440_HOST, "online": False, "reason": "no credentials"}

        # Laptop — overflow
        if self.laptop:
            laptop_online = self.laptop.ping()
            result["nodes"]["laptop"] = {
                "host": LAPTOP_HOST,
                "online": laptop_online,
                "work_dir": LAPTOP_WORK,
                "role": ROLE_OVERFLOW,
            }
        else:
            result["nodes"]["laptop"] = {"host": LAPTOP_HOST, "online": False, "reason": "no credentials"}

        # Extra nodes (Optiplex fleet, etc.)
        for name, node in self.extra_nodes.items():
            try:
                online = node.ping()
                result["nodes"][name] = {"host": node.host, "online": online}
            except Exception as e:
                result["nodes"][name] = {"host": node.host, "online": False, "error": str(e)}

        return result

    def task_pi_slice(self, area_name: str, bbox: list, sources: list,
                      tile_size: int = 1024, target_resolution: float = 10.0,
                      mission_json: str = "") -> dict:
        """Send slicing task to Pi."""
        if not self.pi:
            return {"error": "Pi SSH not configured"}

        cmd = build_pi_slice_task(area_name, bbox, sources, tile_size, target_resolution, mission_json)
        task_start = datetime.now(timezone.utc).isoformat()

        print(f"  📡 [PI] Running slice pipeline for {area_name}...")
        result = self.pi.run(cmd, timeout=7200)  # 2hr timeout for slicing
        result["task"] = "pi_slice"
        result["area"] = area_name
        result["started_at"] = task_start

        self.task_log.append(result)
        if result["exit_code"] == 0:
            print(f"  ✅ [PI] Slicing complete in {result['duration_s']}s")
        else:
            print(f"  ❌ [PI] Slicing failed (exit {result['exit_code']})")
            if result["stderr"]:
                print(f"     {result['stderr'][:500]}")

        return result

    def task_xenon_process(self, delegate: str = "") -> dict:
        """Send CPU processing task to Xenon (when online)."""
        if not self.xenon:
            return {"error": "Xenon SSH not configured"}

        cmd = build_xenon_process_task(delegate=delegate)
        task_start = datetime.now(timezone.utc).isoformat()

        label = delegate if delegate else "all delegates"
        print(f"  📡 [XENON] Processing {label} tiles...")
        result = self.xenon.run(cmd, timeout=7200)
        result["task"] = "xenon_process"
        result["delegate"] = delegate or "all"
        result["started_at"] = task_start

        self.task_log.append(result)
        if result["exit_code"] == 0:
            print(f"  ✅ [XENON] Processing complete in {result['duration_s']}s")
        else:
            print(f"  ❌ [XENON] Processing failed (exit {result['exit_code']})")
            if result["stderr"]:
                print(f"     {result['stderr'][:500]}")

        return result

    def task_i7_process(self, delegate: str = "") -> dict:
        """Send GPU/TPU processing task to i7."""
        if not self.i7:
            return {"error": "i7 SSH not configured"}

        cmd = build_i7_process_task(delegate=delegate)
        task_start = datetime.now(timezone.utc).isoformat()

        label = delegate if delegate else "gpu/tpu/hybrid"
        print(f"  📡 [I7] Processing {label} tiles...")
        result = self.i7.run(cmd, timeout=7200)
        result["task"] = "i7_process"
        result["delegate"] = delegate or "gpu_tpu"
        result["started_at"] = task_start

        self.task_log.append(result)
        if result["exit_code"] == 0:
            print(f"  ✅ [I7] Processing complete in {result['duration_s']}s")
        else:
            print(f"  ❌ [I7] Processing failed (exit {result['exit_code']})")
            if result["stderr"]:
                print(f"     {result['stderr'][:500]}")

        return result

    def dispatch_by_role(self, task_type: str) -> Optional[SSHNode]:
        """Return the best available node for the given task type.

        Routing logic:
          tpu/gpu/hybrid → i7 (primary) → laptop (overflow) → xenon (fallback)
          cpu            → xenon (primary) → laptop (overflow) → i7 (last resort)
          slice          → pi
        """
        role = TASK_ROLE_MAP.get(task_type, ROLE_CPU)

        if role == ROLE_CONDUCTOR:
            return self.pi

        if role == ROLE_GPU_TPU:
            # In research or coding agent mode, prefer the T440 dual-P100 execution node first.
            if (RESEARCH_MODE or CODING_AGENT_MODE) and self.t440 and self.t440.ping():
                print(f"  ⚡ [DISPATCH] Research/coding mode active — routing {task_type} to T440 first")
                return self.t440
            # i7 remains the primary GPU/TPU node for normal operations.
            if self.i7 and self.i7.ping():
                return self.i7
            # If T440 is available and not chosen first, use it as overflow.
            if self.t440 and self.t440.ping():
                print(f"  ⚡ [DISPATCH] Routing {task_type} to T440 overflow")
                return self.t440
            # Laptop as overflow when i7 and T440 are unavailable.
            if self.laptop and self.laptop.ping():
                print(f"  ⚡ [DISPATCH] i7/T440 offline — routing {task_type} to laptop (overflow)")
                return self.laptop
            # Xenon as last resort (when back online)
            if self.xenon and self.xenon.ping():
                print(f"  ⚡ [DISPATCH] Fallback to xenon for {task_type}")
                return self.xenon

        if role == ROLE_CPU:
            # CPU tasks: Xenon first (when online), then laptop overflow, then i7
            if self.xenon and self.xenon.ping():
                return self.xenon
            if self.laptop and self.laptop.ping():
                print(f"  ⚡ [DISPATCH] Xenon offline — routing {task_type} to laptop (overflow)")
                return self.laptop
            if self.i7 and self.i7.ping():
                print(f"  ⚡ [DISPATCH] CPU fallback to i7 for {task_type}")
                return self.i7

        return None

    def get_task_log(self) -> list:
        return self.task_log

    def all_nodes(self) -> dict[str, Optional[SSHNode]]:
        """Return all known nodes (pi, xenon, i7, t440, laptop, plus extras)."""
        nodes = {"pi": self.pi, "xenon": self.xenon, "i7": self.i7, "t440": self.t440, "laptop": self.laptop}
        nodes.update(self.extra_nodes)
        return nodes

    def task_node_run(self, node_name: str, cmd: str, timeout: int = 3600) -> dict:
        """Run an arbitrary command on any registered node by name."""
        nodes = self.all_nodes()
        node = nodes.get(node_name)
        if node is None:
            return {"error": f"Unknown node: {node_name}"}
        task_start = datetime.now(timezone.utc).isoformat()
        result = node.run(cmd, timeout=timeout)
        result["task"] = f"run@{node_name}"
        result["started_at"] = task_start
        self.task_log.append(result)
        return result

    def close(self):
        for node in [self.pi, self.xenon, self.i7, self.t440, self.laptop]:
            if node:
                node.close()
        for node in self.extra_nodes.values():
            node.close()
