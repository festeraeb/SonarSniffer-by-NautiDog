#!/usr/bin/env bash
# Priority BAG pull: H13255 (Cedarville / East Straits) + H13257 (Burns / South Channel).
set -uo pipefail
BASE="https://data.ngdc.noaa.gov/platforms/ocean/nos/coast/H12001-H14000"
SURVEYS=(H13255 H13257)
SUBDIRS=(BAG DR)
OUT="${1:-/data/cesarops/bathymetry/straits_surveys}"
UA="cesarops-straits-priority/1.0"
mkdir -p "$OUT"
log() { echo "[$(date -u +%H:%M:%S)] $*"; }
dl() {
  local url="$1" dest="$2"
  mkdir -p "$(dirname "$dest")"
  local rsize
  rsize=$(curl -sIL -A "$UA" "$url" 2>/dev/null | awk -F': ' 'tolower($1)=="content-length"{v=$2} END{gsub(/\r/,"",v); print v}')
  if [[ -f "$dest" && -n "$rsize" ]]; then
    local lsize; lsize=$(stat -c '%s' "$dest" 2>/dev/null || echo 0)
    if [[ "$lsize" == "$rsize" ]]; then
      log "skip  $(basename "$dest")"
      return 0
    fi
  fi
  log "get   $(basename "$dest")"
  curl -fL --retry 5 --retry-delay 3 -C - -A "$UA" "$url" -o "$dest" || return 1
}
for s in "${SURVEYS[@]}"; do
  log "===== $s ====="
  for d in "${SUBDIRS[@]}"; do
    listing=$(curl -sL -A "$UA" "$BASE/$s/$d/" 2>/dev/null) || continue
    mapfile -t files < <(printf '%s' "$listing" \
      | grep -oE 'href="[^"?][^"]*"' | sed 's/href="//;s/"//' \
      | grep -vE '^(/|https?:|mailto:|\?)' | grep -vE '/$')
    for f in "${files[@]}"; do
      [[ "${SKIP_ELLIPSOID:-1}" == "1" && "$f" == *Ellipsoid* ]] && continue
      dl "$BASE/$s/$d/$f" "$OUT/$s/$d/$f" || true
    done
  done
done
log "done out=$OUT"
du -sh "$OUT" 2>/dev/null || true
