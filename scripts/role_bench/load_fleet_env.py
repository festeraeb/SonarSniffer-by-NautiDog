"""Apply fleet_manifest env (FORGE_URL, REPO, LLM URLs) for role_bench runners."""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path


def apply_fleet_env(repo: Path | None = None) -> Path:
    repo = repo or Path(os.environ.get("REPO", Path(__file__).resolve().parents[2]))
    script = repo / "scripts" / "fleet_manifest.py"
    if not script.is_file():
        return repo
    proc = subprocess.run(
        [sys.executable, str(script), "--repo", str(repo), "env"],
        capture_output=True,
        text=True,
        check=False,
    )
    for line in proc.stdout.splitlines():
        line = line.strip()
        if not line.startswith("export "):
            continue
        body = line[len("export ") :]
        key, _, val = body.partition("=")
        val = val.strip().strip("'")
        os.environ[key] = val
    # Unified LLM slots from manifest (not all exported by fleet_manifest.py env)
    try:
        manifest = json.loads((repo / "config" / "fleet_manifest.json").read_text())
        unified = manifest.get("unified", {})
        llm = unified.get("llm", {})
        mark = Path(os.path.expanduser(unified.get("enabled_mark", "~/.cache/cesarops/fleet-unified")))
        if mark.is_file():
            os.environ.setdefault("T440_LAN", unified.get("t440_lan", "10.0.0.61"))
            for key, url in llm.items():
                if key == "mixtral":
                    os.environ.setdefault("MIXTRAL_URL", url)
                    os.environ.setdefault("THINKER_URL", url)
                elif key == "thinker_rtx":
                    os.environ.setdefault("THINKER_URL", url)
                    os.environ.setdefault("RTX_THINKER_URL", url)
                elif key == "thinker_cpu":
                    os.environ.setdefault("CPU_THINKER_URL", url)
                else:
                    os.environ.setdefault(f"{key.upper()}_URL", url)
    except Exception:
        pass
    return Path(os.environ.get("REPO", str(repo)))


def llm_urls() -> dict[str, str]:
    t440 = os.environ.get("T440_LAN", "10.0.0.61")
    return {
        "gemma": os.environ.get("GEMMA_URL", f"http://{t440}:5001").rstrip("/"),
        "qwen": os.environ.get("QWEN_URL", f"http://{t440}:5002").rstrip("/"),
        "polisher": os.environ.get("POLISHER_URL", f"http://{t440}:5010").rstrip("/"),
        "mixtral": os.environ.get(
            "MIXTRAL_URL", os.environ.get("CPU_URL", f"http://{t440}:5211")
        ).rstrip("/"),
        "rtx": os.environ.get("RTX_URL", "http://127.0.0.1:5200").rstrip("/"),
        "forge": os.environ.get("FORGE_URL", f"http://{t440}:9100").rstrip("/"),
    }
