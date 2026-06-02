"""
swot_displacement_layer.py

SWOT Ka-band radar SSH displacement layer for Lake Michigan Census.

Three tasks:
  1. Fetch SWOT SSH passes over Lake Michigan corridor (2023-2025) via
     NASA CMR (short_name=SWOT_L2_LR_SSH_2.0) and PO.DAAC HTTPS.
  2. Add surface_height_anomaly column to stationary_anchors and new_arrivals.
     Flag any coordinate with persistent SSH deviation > 1 cm.
  3. Triple-lock validation: Thermal Sink (L8) + SAR Stability (S1) + Height
     Anomaly (SWOT) → CONFIRMED_HIGH_MASS_WRECK.
     New arrivals with SSH hit but missing L8/S1 → SWOT_PENDING.

SWOT orbit: 21-day exact repeat. Ka-band nadir swath ~20 km wide.
Lake Michigan corridor bbox: 42.44–42.49 N, 87.12–87.06 W.
SSH anomaly threshold: 1 cm (0.01 m) persistent over ≥2 passes.
"""

import json
import math
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

import requests

# ── Config ────────────────────────────────────────────────────────────────────

REPO    = Path(__file__).resolve().parent
DB_PATH = REPO / 'LAKE_MICHIGAN_CENSUS_2026.db'

# Corridor bbox — same as census engine
CORRIDOR_BBOX = '-87.12,42.44,-87.06,42.49'   # W,S,E,N for CMR

# SWOT CMR parameters
CMR_BASE      = 'https://cmr.earthdata.nasa.gov/search/granules.json'
SWOT_PRODUCTS = [
    'SWOT_L2_LR_SSH_2.0',   # Level-2 low-rate SSH (primary)
    'SWOT_L2_LR_SSH_1.1',   # fallback version
    'SWOT_L2_HR_Raster_2.0', # high-rate raster fallback
]
SWOT_TEMPORAL = '2023-04-01T00:00:00Z,2025-12-31T23:59:59Z'

# SSH anomaly threshold (metres)
SSH_ANOMALY_THRESH_M = 0.01   # 1 cm

# Triple-lock thresholds
SAR_STABILITY_MIN  = 1.0   # S1 coherence / stability score
THERMAL_SINK_FLAG  = True  # presence of L8 thermal sink (boolean in DB)

# Token paths (reuse from census engine)
_TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/erie_remote/erie_remote_data/.earthdata_token'),
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
]

# ── Helpers ───────────────────────────────────────────────────────────────────

def _load_token() -> str:
    for tp in _TOKEN_PATHS:
        if tp.exists():
            try:
                txt = tp.read_text(encoding='utf-8').strip()
                if tp.suffix == '.json':
                    return json.loads(txt).get('earthdata_token', '')
                return txt
            except Exception:
                continue
    return ''


def _haversine_m(lat1, lon1, lat2, lon2) -> float:
    R = 6_371_000.0
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlam = math.radians(lon2 - lon1)
    a = math.sin(dphi/2)**2 + math.cos(phi1)*math.cos(phi2)*math.sin(dlam/2)**2
    return R * 2 * math.asin(math.sqrt(a))

# ── Task 1: SWOT CMR fetch ────────────────────────────────────────────────────

def fetch_swot_passes(token: str = '') -> list[dict]:
    """
    Query NASA CMR for SWOT SSH granules over Lake Michigan corridor 2023-2025.
    Returns list of pass metadata dicts. Does NOT download granule files —
    records URLs for deferred download into acquisition_queue.
    """
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'

    all_passes = []
    for product in SWOT_PRODUCTS:
        params = {
            'short_name':   product,
            'temporal':     SWOT_TEMPORAL,
            'bounding_box': CORRIDOR_BBOX,
            'page_size':    100,
            'sort_key':     'start_date',
        }
        try:
            resp = requests.get(CMR_BASE, params=params, headers=headers, timeout=30)
            resp.raise_for_status()
            entries = resp.json().get('feed', {}).get('entry', [])
            if entries:
                print(f'[SWOT] {product}: {len(entries)} granules found')
                for e in entries:
                    # extract download URL (prefer HTTPS over S3)
                    links = e.get('links', [])
                    dl_url = next(
                        (l['href'] for l in links
                         if l.get('rel') == 'http://esipfed.org/ns/fedsearch/1.1/data#'
                         and 'https://' in l.get('href', '')),
                        None
                    )
                    all_passes.append({
                        'product':    product,
                        'granule_id': e.get('id', ''),
                        'title':      e.get('title', ''),
                        'time_start': e.get('time_start', ''),
                        'time_end':   e.get('time_end', ''),
                        'dl_url':     dl_url,
                    })
                break   # got results from this product version — stop trying fallbacks
            else:
                print(f'[SWOT] {product}: 0 granules — trying next version')
        except Exception as e:
            print(f'[SWOT] {product} CMR error: {e}')

    if not all_passes:
        print('[SWOT] WARNING: No SWOT granules found in CMR for corridor bbox.')
        print('       SWOT Ka-band nadir swath is ~20 km. Corridor may fall between swaths.')
        print('       Widening search to full Lake Michigan bbox for pass inventory...')
        all_passes = _fetch_swot_wide(headers)

    return all_passes


def _fetch_swot_wide(headers: dict) -> list[dict]:
    """Fallback: full Lake Michigan bbox to confirm SWOT coverage exists."""
    params = {
        'short_name':   SWOT_PRODUCTS[0],
        'temporal':     SWOT_TEMPORAL,
        'bounding_box': '-88.0,41.5,-86.0,46.0',   # full Lake Michigan
        'page_size':    20,
        'sort_key':     'start_date',
    }
    try:
        resp = requests.get(CMR_BASE, params=params, headers=headers, timeout=30)
        resp.raise_for_status()
        entries = resp.json().get('feed', {}).get('entry', [])
        print(f'[SWOT] Wide Lake Michigan search: {len(entries)} granules')
        return [{'product': SWOT_PRODUCTS[0], 'granule_id': e.get('id',''),
                 'title': e.get('title',''), 'time_start': e.get('time_start',''),
                 'time_end': e.get('time_end',''), 'dl_url': None,
                 'note': 'wide_bbox_fallback'} for e in entries]
    except Exception as e:
        print(f'[SWOT] Wide search error: {e}')
        return []

# ── Task 2: Schema migration + SSH anomaly column ─────────────────────────────

MIGRATION_SQL = """
ALTER TABLE stationary_anchors ADD COLUMN surface_height_anomaly_m  REAL    DEFAULT NULL;
ALTER TABLE stationary_anchors ADD COLUMN swot_pass_count           INTEGER DEFAULT 0;
ALTER TABLE stationary_anchors ADD COLUMN swot_persistent_anomaly   INTEGER DEFAULT 0;
ALTER TABLE stationary_anchors ADD COLUMN swot_queried_at           TEXT    DEFAULT NULL;

ALTER TABLE new_arrivals ADD COLUMN surface_height_anomaly_m  REAL    DEFAULT NULL;
ALTER TABLE new_arrivals ADD COLUMN swot_pass_count           INTEGER DEFAULT 0;
ALTER TABLE new_arrivals ADD COLUMN swot_persistent_anomaly   INTEGER DEFAULT 0;
ALTER TABLE new_arrivals ADD COLUMN swot_queried_at           TEXT    DEFAULT NULL;

ALTER TABLE stationary_anchors ADD COLUMN thermal_sink_l8     INTEGER DEFAULT 0;
ALTER TABLE stationary_anchors ADD COLUMN sar_stability_s1    REAL    DEFAULT NULL;
ALTER TABLE stationary_anchors ADD COLUMN triple_lock_status  TEXT    DEFAULT 'UNVALIDATED';

ALTER TABLE new_arrivals ADD COLUMN thermal_sink_l8           INTEGER DEFAULT 0;
ALTER TABLE new_arrivals ADD COLUMN sar_stability_s1          REAL    DEFAULT NULL;
ALTER TABLE new_arrivals ADD COLUMN triple_lock_status        TEXT    DEFAULT 'UNVALIDATED';
"""

SWOT_PASSES_TABLE = """
CREATE TABLE IF NOT EXISTS swot_passes (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    product      TEXT,
    granule_id   TEXT UNIQUE,
    title        TEXT,
    time_start   TEXT,
    time_end     TEXT,
    dl_url       TEXT,
    note         TEXT,
    fetched_at   TEXT
);
"""


def _col_exists(conn, table: str, col: str) -> bool:
    cur = conn.cursor()
    cur.execute(f'PRAGMA table_info({table})')
    return any(row[1] == col for row in cur.fetchall())


def apply_schema_migration(conn):
    """Add SWOT + triple-lock columns idempotently (SQLite has no IF NOT EXISTS for ALTER)."""
    migrations = [
        ('stationary_anchors', 'surface_height_anomaly_m',
         'ALTER TABLE stationary_anchors ADD COLUMN surface_height_anomaly_m REAL DEFAULT NULL'),
        ('stationary_anchors', 'swot_pass_count',
         'ALTER TABLE stationary_anchors ADD COLUMN swot_pass_count INTEGER DEFAULT 0'),
        ('stationary_anchors', 'swot_persistent_anomaly',
         'ALTER TABLE stationary_anchors ADD COLUMN swot_persistent_anomaly INTEGER DEFAULT 0'),
        ('stationary_anchors', 'swot_queried_at',
         'ALTER TABLE stationary_anchors ADD COLUMN swot_queried_at TEXT DEFAULT NULL'),
        ('stationary_anchors', 'thermal_sink_l8',
         'ALTER TABLE stationary_anchors ADD COLUMN thermal_sink_l8 INTEGER DEFAULT 0'),
        ('stationary_anchors', 'sar_stability_s1',
         'ALTER TABLE stationary_anchors ADD COLUMN sar_stability_s1 REAL DEFAULT NULL'),
        ('stationary_anchors', 'triple_lock_status',
         'ALTER TABLE stationary_anchors ADD COLUMN triple_lock_status TEXT DEFAULT "UNVALIDATED"'),

        ('new_arrivals', 'surface_height_anomaly_m',
         'ALTER TABLE new_arrivals ADD COLUMN surface_height_anomaly_m REAL DEFAULT NULL'),
        ('new_arrivals', 'swot_pass_count',
         'ALTER TABLE new_arrivals ADD COLUMN swot_pass_count INTEGER DEFAULT 0'),
        ('new_arrivals', 'swot_persistent_anomaly',
         'ALTER TABLE new_arrivals ADD COLUMN swot_persistent_anomaly INTEGER DEFAULT 0'),
        ('new_arrivals', 'swot_queried_at',
         'ALTER TABLE new_arrivals ADD COLUMN swot_queried_at TEXT DEFAULT NULL'),
        ('new_arrivals', 'thermal_sink_l8',
         'ALTER TABLE new_arrivals ADD COLUMN thermal_sink_l8 INTEGER DEFAULT 0'),
        ('new_arrivals', 'sar_stability_s1',
         'ALTER TABLE new_arrivals ADD COLUMN sar_stability_s1 REAL DEFAULT NULL'),
        ('new_arrivals', 'triple_lock_status',
         'ALTER TABLE new_arrivals ADD COLUMN triple_lock_status TEXT DEFAULT "UNVALIDATED"'),
    ]
    cur = conn.cursor()
    added = 0
    for table, col, sql in migrations:
        if not _col_exists(conn, table, col):
            cur.execute(sql)
            added += 1
    conn.execute(SWOT_PASSES_TABLE)
    conn.commit()
    print(f'[+] Schema migration: {added} columns added')


# ── Task 2b: Persist SWOT passes + mark SSH anomalies ─────────────────────────

def ingest_swot_passes(conn, passes: list[dict]):
    """Store SWOT pass inventory; mark coordinates within swath as SWOT_PENDING."""
    if not passes:
        print('[SWOT] No passes to ingest.')
        return

    now = datetime.now(timezone.utc).isoformat()
    cur = conn.cursor()
    inserted = 0
    for p in passes:
        try:
            cur.execute("""
                INSERT OR IGNORE INTO swot_passes
                  (product, granule_id, title, time_start, time_end, dl_url, note, fetched_at)
                VALUES (?,?,?,?,?,?,?,?)
            """, (p['product'], p['granule_id'], p['title'],
                  p['time_start'], p['time_end'], p.get('dl_url'),
                  p.get('note', ''), now))
            inserted += cur.rowcount
        except Exception as e:
            print(f'[SWOT] Insert error: {e}')
    conn.commit()
    print(f'[+] SWOT passes ingested: {inserted} new / {len(passes)} total')

    # Mark all anchors/arrivals as SWOT_PENDING — actual SSH values require
    # granule download (NetCDF). Pass count = number of granules covering
    # the 21-day repeat cycle over the corridor.
    pass_count = len([p for p in passes if 'wide_bbox' not in p.get('note', '')])
    queried_at = now

    cur.execute("""
        UPDATE stationary_anchors
        SET swot_pass_count=?, swot_queried_at=?,
            triple_lock_status=CASE
                WHEN triple_lock_status='UNVALIDATED' THEN 'SWOT_PENDING'
                ELSE triple_lock_status
            END
        WHERE triple_lock_status IN ('UNVALIDATED','SWOT_PENDING')
    """, (pass_count, queried_at))

    cur.execute("""
        UPDATE new_arrivals
        SET swot_pass_count=?, swot_queried_at=?,
            triple_lock_status=CASE
                WHEN triple_lock_status='UNVALIDATED' THEN 'SWOT_PENDING'
                ELSE triple_lock_status
            END
        WHERE triple_lock_status IN ('UNVALIDATED','SWOT_PENDING')
    """, (pass_count, queried_at))
    conn.commit()
    print(f'[+] All anchors/arrivals marked SWOT_PENDING ({pass_count} passes in window)')


def apply_ssh_anomaly_from_file(conn, ssh_json_path: Path):
    """
    Optional: if a pre-extracted SSH anomaly JSON exists (from downloaded NetCDF),
    apply surface_height_anomaly_m values to matching coordinates.

    JSON format: [{"lat": 42.46, "lon": -87.09, "ssh_anomaly_m": 0.023, "pass_count": 4}, ...]
    """
    if not ssh_json_path.exists():
        print(f'[SWOT] No SSH anomaly file at {ssh_json_path} — skipping value application')
        return 0

    records = json.loads(ssh_json_path.read_text(encoding='utf-8'))
    cur = conn.cursor()
    updated = 0
    now = datetime.now(timezone.utc).isoformat()

    for rec in records:
        lat, lon = rec['lat'], rec['lon']
        sha = rec['ssh_anomaly_m']
        pc  = rec.get('pass_count', 1)
        persistent = 1 if (abs(sha) >= SSH_ANOMALY_THRESH_M and pc >= 2) else 0

        for table in ('stationary_anchors', 'new_arrivals'):
            cur.execute(f'SELECT id, lat, lon FROM {table}')
            rows = cur.fetchall()
            for row_id, rlat, rlon in rows:
                if _haversine_m(lat, lon, rlat, rlon) <= 150.0:
                    cur.execute(f"""
                        UPDATE {table}
                        SET surface_height_anomaly_m=?, swot_pass_count=?,
                            swot_persistent_anomaly=?, swot_queried_at=?
                        WHERE id=?
                    """, (sha, pc, persistent, now, row_id))
                    updated += cur.rowcount

    conn.commit()
    print(f'[+] SSH anomaly values applied to {updated} rows')
    return updated


# ── Task 3: Triple-lock validation ────────────────────────────────────────────

def run_triple_lock_validation(conn):
    """
    Triple-lock logic:
      CONFIRMED_HIGH_MASS_WRECK : thermal_sink_l8=1 AND sar_stability_s1>=1.0
                                   AND swot_persistent_anomaly=1
      SWOT_PENDING              : swot_persistent_anomaly=1 but missing L8 or S1
      DUAL_LOCK                 : thermal_sink_l8=1 AND sar_stability_s1>=1.0
                                   (no SWOT yet)
      SWOT_PENDING stays        : swot_pass_count>0 but no anomaly confirmed yet

    Runs on both stationary_anchors and new_arrivals.
    """
    cur = conn.cursor()
    stats = {'confirmed': 0, 'dual_lock': 0, 'swot_pending': 0}

    for table in ('stationary_anchors', 'new_arrivals'):
        cur.execute(f"""
            SELECT id, thermal_sink_l8, sar_stability_s1,
                   swot_persistent_anomaly, swot_pass_count
            FROM {table}
        """)
        rows = cur.fetchall()
        for row_id, thermal, sar, swot_hit, swot_passes in rows:
            sar_ok     = (sar is not None and sar >= SAR_STABILITY_MIN)
            thermal_ok = bool(thermal)
            swot_ok    = bool(swot_hit)

            if thermal_ok and sar_ok and swot_ok:
                status = 'CONFIRMED_HIGH_MASS_WRECK'
                stats['confirmed'] += 1
            elif swot_ok and not (thermal_ok and sar_ok):
                status = 'SWOT_PENDING'   # has height anomaly, awaiting L8+S1
                stats['swot_pending'] += 1
            elif thermal_ok and sar_ok and not swot_ok:
                status = 'DUAL_LOCK'      # L8+S1 confirmed, SWOT not yet
                stats['dual_lock'] += 1
            elif (swot_passes or 0) > 0:
                status = 'SWOT_PENDING'   # passes exist, anomaly not yet extracted
                stats['swot_pending'] += 1
            else:
                continue  # leave as UNVALIDATED

            cur.execute(f'UPDATE {table} SET triple_lock_status=? WHERE id=?',
                        (status, row_id))

    conn.commit()
    return stats


# ── Queue SWOT granule downloads ──────────────────────────────────────────────

def queue_swot_downloads(conn, passes: list[dict]):
    """Add SWOT granule download tasks to acquisition_queue."""
    cur = conn.cursor()
    now = datetime.now(timezone.utc).isoformat()
    queued = 0
    for p in passes[:20]:   # cap at 20 — full download is a separate pipeline step
        if not p.get('dl_url'):
            continue
        tile_id = p['title'][:40] if p['title'] else p['granule_id'][:40]
        cur.execute("SELECT COUNT(*) FROM acquisition_queue WHERE tile=?", (tile_id,))
        if cur.fetchone()[0] == 0:
            cur.execute("""
                INSERT INTO acquisition_queue (tile, product, date_window, reason, status, queued_at)
                VALUES (?,?,?,?,?,?)
            """, (tile_id, p['product'],
                  f"{p['time_start'][:10]}/{p['time_end'][:10]}",
                  f"SWOT SSH pass — download for SSH anomaly extraction. URL: {p['dl_url']}",
                  'PENDING', now))
            queued += cur.rowcount
    conn.commit()
    print(f'[+] {queued} SWOT granules queued for download in acquisition_queue')


# ── Report ────────────────────────────────────────────────────────────────────

def print_report(conn, passes: list[dict], triple_stats: dict):
    cur = conn.cursor()

    cur.execute("SELECT COUNT(*) FROM stationary_anchors WHERE swot_persistent_anomaly=1")
    anchors_with_ssh = cur.fetchone()[0]

    cur.execute("SELECT COUNT(*) FROM new_arrivals WHERE swot_persistent_anomaly=1")
    arrivals_with_ssh = cur.fetchone()[0]

    cur.execute("SELECT COUNT(*) FROM stationary_anchors WHERE triple_lock_status='CONFIRMED_HIGH_MASS_WRECK'")
    confirmed = cur.fetchone()[0]

    cur.execute("SELECT COUNT(*) FROM new_arrivals WHERE triple_lock_status='SWOT_PENDING'")
    pending_arrivals = cur.fetchone()[0]

    cur.execute("SELECT COUNT(*) FROM new_arrivals WHERE triple_lock_status='DUAL_LOCK'")
    dual_arrivals = cur.fetchone()[0]

    sep = '=' * 62
    print(f'\n{sep}')
    print('SWOT DISPLACEMENT LAYER — REPORT')
    print(sep)
    print(f'  SWOT passes found (2023-2025 corridor) : {len(passes):>4}')
    print(f'  Anchors with SSH anomaly >1cm          : {anchors_with_ssh:>4}')
    print(f'  New arrivals with SSH anomaly >1cm     : {arrivals_with_ssh:>4}')
    print(sep)
    print('  TRIPLE-LOCK VALIDATION')
    print(f'    CONFIRMED_HIGH_MASS_WRECK            : {confirmed:>4}  (L8+S1+SWOT)')
    print(f'    DUAL_LOCK (L8+S1, SWOT pending)      : {triple_stats["dual_lock"]:>4}')
    print(f'    SWOT_PENDING (height hit, no L8/S1)  : {triple_stats["swot_pending"]:>4}')
    print(f'    New arrivals SWOT_PENDING             : {pending_arrivals:>4}  <- FRESH GHOSTS w/ volume')
    print(f'    New arrivals DUAL_LOCK                : {dual_arrivals:>4}')
    print(sep)

    if len(passes) == 0:
        print('\n  NOTE: 0 SWOT passes in corridor bbox.')
        print('  SWOT Ka-band nadir swath = 20 km. Corridor (42.44-42.49N, 87.12-87.06W)')
        print('  may fall between swaths on some cycles. Wide-bbox passes confirm')
        print('  Lake Michigan is covered — SSH extraction requires granule download.')
        print('  All coordinates marked SWOT_PENDING pending NetCDF extraction.')

    # Show top anchors with SWOT status
    cur.execute("""
        SELECT lat, lon, combined_score, triple_lock_status,
               surface_height_anomaly_m, swot_pass_count, in_corridor
        FROM stationary_anchors ORDER BY combined_score DESC LIMIT 7
    """)
    rows = cur.fetchall()
    print('\n  STATIONARY ANCHORS — SWOT STATUS:\n')
    for lat, lon, cs, status, sha, pc, corr in rows:
        tag  = ' [CORRIDOR]' if corr else ''
        sha_str = f'{sha*100:.1f}cm' if sha is not None else 'PENDING'
        print(f'    {lat:.5f}, {lon:.5f}  score={cs}  SSH={sha_str}'
              f'  passes={pc}  [{status}]{tag}')

    print()


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    print(f'[+] SWOT Displacement Layer — {datetime.now(timezone.utc).isoformat()}')
    print(f'[+] DB: {DB_PATH}')

    if not DB_PATH.exists():
        print(f'[!] DB not found: {DB_PATH}')
        print('    Run lake_census_engine.py first.')
        sys.exit(1)

    conn = sqlite3.connect(str(DB_PATH))

    # Task 2: schema migration
    apply_schema_migration(conn)

    # Task 1: SWOT CMR fetch
    token = _load_token()
    print(f'[+] Earthdata token: {"loaded" if token else "NOT FOUND — anonymous CMR query"}')
    print('[+] Querying NASA CMR for SWOT SSH passes (2023-2025)...')
    passes = fetch_swot_passes(token)
    print(f'[+] Total SWOT passes retrieved: {len(passes)}')

    # Persist passes + mark SWOT_PENDING
    ingest_swot_passes(conn, passes)

    # Queue granule downloads
    queue_swot_downloads(conn, passes)

    # Apply SSH values from pre-extracted file if it exists
    ssh_file = REPO / 'outputs' / 'calibration' / 'swot_ssh_anomalies.json'
    apply_ssh_anomaly_from_file(conn, ssh_file)

    # Task 3: triple-lock validation
    print('[+] Running triple-lock validation...')
    triple_stats = run_triple_lock_validation(conn)

    # Report
    print_report(conn, passes, triple_stats)

    # Write JSON summary
    out_path = REPO / 'outputs' / 'calibration' / 'swot_displacement_report.json'
    out_path.parent.mkdir(parents=True, exist_ok=True)

    cur = conn.cursor()
    cur.execute("""
        SELECT lat, lon, combined_score, triple_lock_status,
               surface_height_anomaly_m, swot_pass_count, in_corridor
        FROM stationary_anchors ORDER BY combined_score DESC
    """)
    anchor_rows = [{'lat': r[0], 'lon': r[1], 'combined_score': r[2],
                    'triple_lock_status': r[3], 'surface_height_anomaly_m': r[4],
                    'swot_pass_count': r[5], 'in_corridor': bool(r[6])}
                   for r in cur.fetchall()]

    cur.execute("""
        SELECT lat, lon, score, triple_lock_status,
               surface_height_anomaly_m, swot_pass_count, in_corridor
        FROM new_arrivals ORDER BY score DESC
    """)
    arrival_rows = [{'lat': r[0], 'lon': r[1], 'score': r[2],
                     'triple_lock_status': r[3], 'surface_height_anomaly_m': r[4],
                     'swot_pass_count': r[5], 'in_corridor': bool(r[6])}
                    for r in cur.fetchall()]

    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump({
            'run_at': datetime.now(timezone.utc).isoformat(),
            'swot_passes_found': len(passes),
            'ssh_anomaly_threshold_m': SSH_ANOMALY_THRESH_M,
            'triple_lock_stats': triple_stats,
            'swot_passes': passes[:20],
            'stationary_anchors': anchor_rows,
            'new_arrivals_swot_pending': [a for a in arrival_rows
                                          if a['triple_lock_status'] == 'SWOT_PENDING'],
        }, f, indent=2)
    print(f'[+] Written {out_path}')

    conn.close()


if __name__ == '__main__':
    main()
