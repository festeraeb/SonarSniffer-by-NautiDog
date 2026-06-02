#!/usr/bin/env bash
# Symlink healthy repo slices into var/recovery/clean_repos for P106 embedder baselines.
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
# Do not use generic OUT from env (Forge bench dirs hijack it).
CLEAN_REPOS_OUT="${CLEAN_REPOS:-$REPO/var/recovery/clean_repos}"
OUT="$CLEAN_REPOS_OUT"
mkdir -p "$OUT"

link_tree() {
  local name="$1"
  local src="$2"
  [[ -d "$src" ]] || { echo "skip $name — missing $src"; return 0; }
  local dest="$OUT/$name"
  rm -f "$dest" 2>/dev/null || true
  ln -sfn "$src" "$dest"
  echo "linked $name -> $src"
}

link_tree "wreckhunter2000-1" "$REPO"
link_tree "cesarops-forge-v2" "$REPO/cesarops-forge-v2"
link_tree "cesarops-detection" "$REPO/cesarops-detection"
link_tree "infra-nomad" "$REPO/infra/nomad"
link_tree "scripts-fleet" "$REPO/scripts"

# Optional: small recovery docs from laptop dump zips (read-only reference)
for ld in /data/laptopdump /mnt/t440/data/laptopdump; do
  if [[ -d "$ld/programming" ]]; then
    link_tree "laptopdump-programming" "$ld/programming"
    break
  fi
done

echo "clean_repos ready at $OUT"
