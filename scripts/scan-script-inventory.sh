#!/usr/bin/env bash
# Inventory .sh files for Forge cleanup mission. No deletes.
# Usage: scan-script-inventory.sh [--out var/script-inventory/latest]
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
[[ -d /mnt/t440/repo ]] && REPO="/mnt/t440/repo"
OUT="${1:-}"
if [[ "${1:-}" == "--out" ]]; then
  OUT="${2:?}"
fi
OUT="${OUT:-${REPO}/var/script-inventory/latest}"
HOST="$(hostname -s 2>/dev/null || echo unknown)"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

mkdir -p "$OUT/by-tier"

# Heuristic tier guess (Forge overrides in forge-verdict.json)
guess_tier() {
  local rel="$1"
  case "$rel" in
    scripts/lib/*|infra/*|systemd/*|cesarops-forge-v2/scripts/*)
      echo keep
      ;;
    */fleet-*|*/forge-health-*|*/forge-sync-*|*/forge_apply_*|*/cluster-*|*/n8n-watchdog*|*/mission_service*|*/start_n8n*|*/deploy_forge*|*/build_sonarsniffer*|*/setup_cesarops_tunnel*|*/credentials.sh|*/ensure_n8n*|*/rclone-*|*/setup_t440_nfs*)
      echo keep
      ;;
    */t440-disable*|*/download_model_candidates_*|*/fleet-job-runner.sh|*/integrate/run_*_dispatch*|*/cake/install*|*/cake/prep*|*/cake/patch*|*/cake/diagnose*|*/cake/run-qwen*|*/forge_run_until*|*/forge_overnight*)
      echo review
      ;;
    */_ephemeral/*|*oneoff*|*one-off*)
      echo ephemeral
      ;;
    *)
      echo review
      ;;
  esac
}

MANIFEST="${OUT}/manifest.json"
REPORT="${OUT}/report.md"
: >"${OUT}/by-tier/keep.txt"
: >"${OUT}/by-tier/review.txt"
: >"${OUT}/by-tier/ephemeral.txt"

mapfile -t FILES < <(
  {
    find "$REPO/scripts" "$REPO/infra" "$REPO/systemd" "$REPO/config" \
      "$REPO/cesarops-forge-v2/scripts" -name '*.sh' -type f 2>/dev/null
    find "$REPO" -maxdepth 1 -name '*.sh' -type f 2>/dev/null
  } | grep -vE '/\.git/|/target/|/node_modules/|\.venv|/backup/|/integrate_out/|\.cargo' \
    | sort -u
)

python3 - "$REPO" "$HOST" "$TS" "$MANIFEST" "${FILES[@]}" <<'PY'
import json, os, re, sys
repo, host, ts, out = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
files = sys.argv[5:]

def guess(rel):
    r = rel.replace("\\", "/")
    if any(x in r for x in ("scripts/lib/", "infra/", "systemd/", "cesarops-forge-v2/scripts/")):
        return "keep"
    keep_pat = (
        "fleet-", "forge-health", "forge-sync", "forge_apply_", "cluster-",
        "n8n-watchdog", "mission_service", "start_n8n", "deploy_forge",
        "build_sonarsniffer", "setup_cesarops_tunnel", "credentials.sh",
        "ensure_n8n", "rclone-", "setup_t440_nfs", "forge-routing",
    )
    if any(p in r for p in keep_pat):
        return "keep"
    rev_pat = (
        "t440-disable", "download_model_candidates_", "/fleet-job-runner.sh",
        "integrate/run_", "cake/install", "cake/prep", "cake/patch",
        "cake/diagnose", "forge_run_until", "forge_overnight",
        "cesarops2_fr", "deploy_configs_to_cesarops2_fr",
    )
    if any(p in r for p in rev_pat):
        return "review"
    if "_ephemeral/" in r or "oneoff" in r.lower():
        return "ephemeral"
    return "review"

rows = []
for path in files:
    rel = os.path.relpath(path, repo)
    st = os.stat(path)
    head = ""
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            head = "".join(f.readline() for _ in range(5))
    except OSError:
        pass
    ephemeral_tag = "CESAROPS_EPHEMERAL=1" in head
    tier = "ephemeral" if ephemeral_tag else guess(rel)
    rows.append({
        "path": rel,
        "abs_path": path,
        "host_scan": host,
        "bytes": st.st_size,
        "mtime_utc": ts,
        "tier_guess": tier,
        "ephemeral_header": ephemeral_tag,
        "duplicate_of": None,
    })

# Mark duplicate: scripts/foo vs foo at repo root
by_name = {}
for r in rows:
    base = os.path.basename(r["path"])
    by_name.setdefault(base, []).append(r["path"])
for base, paths in by_name.items():
    if len(paths) > 1:
        canonical = min(paths, key=len)
        for p in paths:
            if p != canonical:
                for r in rows:
                    if r["path"] == p:
                        r["duplicate_of"] = canonical

doc = {"scanned_at": ts, "host": host, "repo": repo, "count": len(rows), "scripts": rows}
with open(out, "w", encoding="utf-8") as f:
    json.dump(doc, f, indent=2)
print(f"wrote {out} ({len(rows)} scripts)")
PY

# Tier lists + markdown report
python3 - "$MANIFEST" "$REPORT" "$OUT" <<'PY'
import json, sys
from collections import Counter
manifest, report, out = sys.argv[1], sys.argv[2], sys.argv[3]
data = json.load(open(manifest))
counts = Counter(r["tier_guess"] for r in data["scripts"])
by_tier = {t: [] for t in ("keep", "review", "ephemeral")}
for r in data["scripts"]:
    by_tier[r["tier_guess"]].append(r["path"])
    open(f"{out}/by-tier/{r['tier_guess']}.txt", "a").write(r["path"] + "\n")
dups = [r for r in data["scripts"] if r.get("duplicate_of")]
lines = [
    f"# Script inventory — {data['host']}",
    f"Scanned: {data['scanned_at']}  Repo: `{data['repo']}`",
    f"Total: **{data['count']}** shell scripts",
    "",
    "## Tier counts (heuristic — Forge must confirm)",
    "",
    f"| Tier | Count |",
    f"|------|-------|",
]
for t in ("keep", "review", "ephemeral"):
    lines.append(f"| {t} | {counts.get(t, 0)} |")
lines += ["", "## Duplicates (same basename)", ""]
for r in dups[:40]:
    lines.append(f"- `{r['path']}` → `{r['duplicate_of']}`")
if len(dups) > 40:
    lines.append(f"- … and {len(dups) - 40} more")
lines += ["", "## Review tier (sample)", ""]
for p in by_tier["review"][:60]:
    lines.append(f"- `{p}`")
if len(by_tier["review"]) > 60:
    lines.append(f"- … {len(by_tier['review']) - 60} more")
open(report, "w").write("\n".join(lines) + "\n")
print(f"wrote {report}")
PY

echo "[scan-script-inventory] host=$HOST out=$OUT"
wc -l "$OUT/by-tier/"*.txt
