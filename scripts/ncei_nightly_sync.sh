#!/usr/bin/env bash
# Nightly NCEI Great Lakes survey sync.
#
# Re-crawls the NOS coast index and downloads only NEW/missing products for
# Great-Lakes surveys. Files already present at the server's content-length are
# skipped (the scraper's size-match check), so this is incremental: existing
# downloads are ignored, only genuinely new survey products are fetched.
#
# Driven by the 'detect' product preset:
#   BAG + backscatter (TIFF/MBAB) + DR (pdf/xml) + GEODAS + Bottom_Samples
#
# Usage:
#   scripts/ncei_nightly_sync.sh                # default greatlakes, detect preset
#   PRODUCTS=bag scripts/ncei_nightly_sync.sh   # bathy only
#   MISSION=general scripts/ncei_nightly_sync.sh # whole H range (heavy)
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO" || exit 1

OUT="${OUT:-/data/cesarops/bathymetry/ncei_coast}"
MISSION="${MISSION:-greatlakes}"
PRODUCTS="${PRODUCTS:-detect}"
LOGDIR="${LOGDIR:-/data/cesarops/bathymetry/ncei_sync_logs}"
mkdir -p "$LOGDIR" "$OUT"

ts="$(date -u +%Y%m%dT%H%M%SZ)"
log="$LOGDIR/nightly_${ts}.log"

# Single-instance guard: don't stack runs if a previous night is still going.
lock="$LOGDIR/.nightly.lock"
exec 9>"$lock"
if ! flock -n 9; then
  echo "[$(date -u)] another ncei_nightly_sync is already running; exiting" >> "$log"
  exit 0
fi

echo "[$(date -u)] start mission=$MISSION products=$PRODUCTS out=$OUT" | tee -a "$log"

python3 scripts/noaa_ncei_coast_scraper.py \
  --include-prefix H \
  --mission "$MISSION" \
  --products "$PRODUCTS" \
  --output-root "$OUT" >> "$log" 2>&1
rc=$?

echo "[$(date -u)] done rc=$rc" | tee -a "$log"

# Retain only the last 30 nightly logs.
ls -1t "$LOGDIR"/nightly_*.log 2>/dev/null | tail -n +31 | xargs -r rm -f

exit $rc
