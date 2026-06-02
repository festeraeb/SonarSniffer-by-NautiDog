#!/usr/bin/env bash
# Patch cluster_config.toml with cesarops2-fr Tailscale IP (run on T440).
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
CFG="${REPO}/cesarops-forge-v2/cluster_config.toml"
FR_IP="${1:?cesarops2-fr Tailscale IPv4 (tailscale ip -4 on FR host)}"

python3 <<PY
import pathlib, re
ip = "${FR_IP}"
path = pathlib.Path("${CFG}")
text = path.read_text()
marker = "[[known_nodes]]\nname = \"Cesarops2-FR"
if "Cesarops2-FR" not in text:
    snippet = pathlib.Path("${REPO}/config/cesarops2-fr/cluster_config.snippet.toml").read_text()
    snippet = snippet.replace("CESAROPS2_FR_TS_IP", ip)
    # insert after first cesarops2 known_nodes block
    text = text.replace(
        'notes = "LAN 10.0.0.201 / Tailscale 100.102.158.111',
        'notes = "LAN 10.0.0.201 / Tailscale 100.102.158.111 (legacy ML350e — see Cesarops2-FR)"',
        1,
    )
    insert_at = text.find("\n[[known_nodes]]\ngpu = \"GTX 1060")
    if insert_at < 0:
        insert_at = len(text)
    text = text[:insert_at] + "\n" + snippet.strip() + "\n" + text[insert_at:]
# Add FR URLs to pools if missing
for pool in ("mtp", "intake"):
    block = re.search(rf"\\[endpoint_pool\\.{pool}\\][^\\[]*", text, re.S)
    if block:
        urls = f'    "http://{ip}:5571",\\n    "http://{ip}:5200",\\n'
        if f"http://{ip}:5571" not in block.group(0):
            text = text.replace(
                f"[endpoint_pool.{pool}]\nurls = [",
                f"[endpoint_pool.{pool}]\nurls = [\n{urls}",
                1,
            )
path.write_text(text)
print(f"Patched {path} with cesarops2-fr IP {ip}")
PY
