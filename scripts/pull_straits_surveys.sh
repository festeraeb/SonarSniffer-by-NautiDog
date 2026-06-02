#!/usr/bin/env bash
# Targeted pull of the 7 Straits of Mackinac NOS hydrographic surveys.
#
# These are the primary AOI surveys (Andaste / Straits cold-sink work):
#   H13252  West Straits of Mackinac
#   H13253  Grays Reef Passage
#   H13255  East Straits of Mackinac
#   H13256  vicinity of Ile Aux Galets
#   H13257  South Channel
#   H13258  regional hydrographic operations
#   H13259  a few miles north of Garden Island
#
# Grabs every product subfolder that matters for the detector + seam work:
#   BAG/            primary bathymetry (elevation + uncertainty)
#   MBAB/           multibeam acoustic backscatter mosaic (per-vessel = seam info)
#   DR/             descriptive report (pdf + structured xml) -> survey-line/seam metadata
#   GeoImagePDF/    georeferenced image pdf
#   Bottom_Samples/ sediment grabs -> ground-truth "natural vs masked flat zone"
#
# Resume-safe: skips files already present at the server's size.
set -uo pipefail

BASE="https://data.ngdc.noaa.gov/platforms/ocean/nos/coast/H12001-H14000"
SURVEYS=(H13252 H13253 H13255 H13256 H13257 H13258 H13259)
SUBDIRS=(BAG MBAB DR GeoImagePDF Bottom_Samples)
OUT="${1:-/data/cesarops/bathymetry/straits_surveys}"
UA="cesarops-straits-puller/1.0"

mkdir -p "$OUT"
log() { echo "[$(date -u +%H:%M:%S)] $*"; }

dl() { # url dest
  local url="$1" dest="$2"
  mkdir -p "$(dirname "$dest")"
  # remote size (follow redirects, last Content-Length wins)
  local rsize
  rsize=$(curl -sIL -A "$UA" "$url" 2>/dev/null | awk -F': ' 'tolower($1)=="content-length"{v=$2} END{gsub(/\r/,"",v); print v}')
  if [[ -f "$dest" && -n "$rsize" ]]; then
    local lsize; lsize=$(stat -c '%s' "$dest" 2>/dev/null || echo 0)
    if [[ "$lsize" == "$rsize" ]]; then
      log "skip  $(basename "$dest") (size match $rsize)"
      return 0
    fi
  fi
  log "get   $(basename "$dest") ${rsize:+($rsize bytes)}"
  # -C - resumes a partial; --retry handles transient drops
  curl -fL --retry 5 --retry-delay 3 -C - -A "$UA" "$url" -o "$dest" \
    || { log "FAIL  $url"; return 1; }
}

total_ok=0 total_fail=0
for s in "${SURVEYS[@]}"; do
  log "===== survey $s ====="
  for d in "${SUBDIRS[@]}"; do
    listing=$(curl -sL -A "$UA" "$BASE/$s/$d/" 2>/dev/null) || continue
    # extract file hrefs (skip parent dir, query-sorted links, absolute links)
    mapfile -t files < <(printf '%s' "$listing" \
      | grep -oE 'href="[^"?][^"]*"' | sed 's/href="//;s/"//' \
      | grep -vE '^(/|https?:|mailto:|\?)' | grep -vE '/$')
    [[ ${#files[@]} -eq 0 ]] && { log "(no files in $s/$d)"; continue; }
    for f in "${files[@]}"; do
      # Skip ellipsoid-datum BAGs: LWD (chart datum) and Ellipsoid produce
      # identical detections, so keep only LWD and avoid the duplicate volume.
      if [[ "${SKIP_ELLIPSOID:-1}" == "1" && "$f" == *Ellipsoid* ]]; then
        log "skip  $f (ellipsoid dup; keeping LWD)"
        continue
      fi
      if dl "$BASE/$s/$d/$f" "$OUT/$s/$d/$f"; then
        total_ok=$((total_ok+1))
      else
        total_fail=$((total_fail+1))
      fi
    done
  done
done

log "===== done: ok=$total_ok fail=$total_fail out=$OUT ====="
du -sh "$OUT" 2>/dev/null
