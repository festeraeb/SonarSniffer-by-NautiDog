#!/usr/bin/env python3
"""Read config/fleet_manifest.json — resolve repo, host role, export env for fleet CLI."""
from __future__ import annotations

import json
import os
import socket
import sys
from pathlib import Path
from typing import Any


def _repo_from_file() -> Path | None:
    try:
        p = Path(__file__).resolve()
        for parent in p.parents:
            if (parent / "config" / "fleet_manifest.json").is_file():
                return parent
    except Exception:
        pass
    return None


def load_manifest(repo: Path | None = None) -> tuple[Path, dict[str, Any]]:
    if repo is None:
        env_repo = os.environ.get("REPO", "").strip()
        if env_repo:
            repo = Path(env_repo)
        else:
            found = _repo_from_file()
            if found is None:
                raise FileNotFoundError("fleet_manifest.json not found; set REPO")
            repo = found
    repo = repo.resolve()
    path = repo / "config" / "fleet_manifest.json"
    if not path.is_file():
        raise FileNotFoundError(path)
    return repo, json.loads(path.read_text(encoding="utf-8"))


def resolve_repo(manifest: dict[str, Any]) -> Path:
    for cand in manifest.get("paths", {}).get("repo_candidates", []):
        p = Path(cand)
        if (p / "config" / "fleet_manifest.json").is_file():
            return p.resolve()
    raise FileNotFoundError("no repo candidate with fleet_manifest.json")


def hostname_node() -> str:
    hn = socket.gethostname().split(".")[0].lower()
    if "t440" in hn:
        return "t440"
    return "cesarops2"


def host_config(manifest: dict[str, Any], node: str | None = None) -> dict[str, Any]:
    node = node or hostname_node()
    hosts = manifest.get("hosts", {})
    if node in hosts:
        return hosts[node]
    return hosts.get("cesarops2", {})


def forge_url(manifest: dict[str, Any], node: str | None = None) -> str:
    node = node or hostname_node()
    hc = host_config(manifest, node)
    if node == "t440" and os.environ.get("T440_RECOVERY") == "1":
        return hc.get("forge_url_recovery_only", "http://10.0.0.61:9100")
    if node == "cesarops2":
        return hc.get("forge_url", "http://127.0.0.1:9100")
    return os.environ.get("FORGE_URL", "http://127.0.0.1:9100")


def n8n_user_folder(manifest: dict[str, Any]) -> str:
    for cand in manifest.get("paths", {}).get("n8n_user_folder_candidates", []):
        p = Path(cand)
        if (p / "database.sqlite").is_file():
            return str(p)
    return str(Path.home() / ".n8n")


def export_env(repo: Path, manifest: dict[str, Any], node: str | None = None) -> dict[str, str]:
    node = node or hostname_node()
    hc = host_config(manifest, node)
    paths = manifest.get("paths", {})
    out = {
        "REPO": str(repo),
        "FLEET_NODE": node,
        "FORGE_URL": forge_url(manifest, node),
        "N8N_URL": hc.get("n8n_url", "http://127.0.0.1:5678"),
        "FLEET_CATALOG_DIR": str(repo / paths.get("catalog_dir", "var/fleet-catalog")),
        "N8N_USER_FOLDER": n8n_user_folder(manifest),
        "FLEET_MANIFEST": str(repo / "config" / "fleet_manifest.json"),
    }
    models = paths.get("models")
    if models:
        out["MODELS"] = models
    return out


def shell_export(env: dict[str, str]) -> str:
    lines = []
    for k, v in sorted(env.items()):
        v = str(v).replace("'", "'\\''")
        lines.append(f"export {k}='{v}'")
    return "\n".join(lines)


def main() -> int:
    import argparse

    ap = argparse.ArgumentParser(description="Fleet manifest resolver")
    ap.add_argument("--repo", type=Path, default=None)
    ap.add_argument("--node", default=None)
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("env", help="Print shell export lines")
    p_get = sub.add_parser("get", help="Get a manifest key (dotted)")
    p_get.add_argument("key")
    sub.add_parser("json", help="Print resolved manifest + repo")
    sub.add_parser("host", help="Print normalized host id")

    args = ap.parse_args()
    try:
        repo, manifest = load_manifest(args.repo)
        repo = resolve_repo(manifest)
    except FileNotFoundError as e:
        print(e, file=sys.stderr)
        return 1

    node = args.node or hostname_node()

    if args.cmd == "host":
        print(node)
        return 0
    if args.cmd == "env":
        print(shell_export(export_env(repo, manifest, node)))
        return 0
    if args.cmd == "json":
        payload = {
            "repo": str(repo),
            "node": node,
            "env": export_env(repo, manifest, node),
            "host": host_config(manifest, node),
            "services": manifest.get("services", {}),
            "llm_slots": manifest.get("llm_slots", {}).get(node, []),
        }
        print(json.dumps(payload, indent=2))
        return 0
    if args.cmd == "get":
        cur: Any = manifest
        for part in args.key.split("."):
            if isinstance(cur, dict) and part in cur:
                cur = cur[part]
            else:
                print("", end="")
                return 1
        if isinstance(cur, (dict, list)):
            print(json.dumps(cur))
        else:
            print(cur)
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
