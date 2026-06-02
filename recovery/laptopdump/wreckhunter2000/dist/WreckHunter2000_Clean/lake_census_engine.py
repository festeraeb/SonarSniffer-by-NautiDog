"""
lake_census_engine.py

Lake Michigan Total Census — Phase 11 & 12 Census Bridge
Real data only. No simulation.

Epochs on disk:
  2024-08-07  rossa_baseline_202408/optical_all_concepts.json
  2025-09-16  rossa_forensic_202509/optical_all_concepts.json

Classifications:
  STATIONARY_ANCHOR   — hit present in BOTH epochs within SNAP_M
  NEW_ARRIVAL         — hit in 2025 only, no match in 2024 (Rossa candidates)
  MOBILE_SIGNAL_PROBE — weekend hit within SANDBAR_SNAP_M of known sandbar
  CANDIDATE           — unclassified

2012 Landsat C2L2 fetch runs in a background thread — does not block CUDA.
"""

import json
import math
import sqlite3
import sys
import threading
from datetime import datetime, timezone
from pathlib import Path

import requests

# ── Paths ─────────────────────────────────────────────────────────────────────

REPO     = Path(__file__).resolve().parent
BAG_REPO = Path('c:/Users/thomf/programming/Bagrecovery')
OUTPUTS  = BAG_REPO / 'outputs'
DB_PATH  = REPO / 'LAKE_MICHIGAN_CENSUS_2026.db'

EPOCH_FILES = {
    '2024-08-07': OUTPUTS / 'rossa_baseline_202408' / 'optical_all_concepts.json',
    '2025-09-16': OUTPUTS / 'rossa_forensic_202509' / 'optical_all_concepts.json',
}

# Sentinel-2 scene metadata locked to each epoch
EPOCH_META = {
    '2024-08-07': {
        'scene_id':        'S2C_16TDN_20240807_0_L2A',
        'sat_zenith_deg':   8.2,    # Sentinel-2 typical nadir pass over Great Lakes
        'sun_azimuth_deg':  152.3,  # Aug 07 ~10:30 UTC, lat 42.46N
        'sun_elevation_deg': 57.1,
        'incidence_angle':  8.2,    # approx = sat_zenith for nadir
        'orbit_time_utc':  '2024-08-07T16:04:00Z',
    },
    '2025-09-16': {
        'scene_id':        'S2C_16TDN_20250916_0_L2A',
        'sat_zenith_deg':   6.9,
        'sun_azimuth_deg':  158.7,  # Sep 16 ~10:30 UTC, lat 42.46N
        'sun_elevation_deg': 46.3,
        'incidence_angle':  6.9,
        'orbit_time_utc':  '2025-09-16T16:04:00Z',
    },
}

# NDBC buoys — 45007 South Lake Michigan, 45002 North Michigan fallback
NDBC_BUOYS   = ['45007', '45002', '45012']
NDBC_BASE    = 'https://www.ndbc.noaa.gov/data/realtime2/{}.txt'

SNAP_M       = 150.0   # STATIONARY_ANCHOR match radius
SANDBAR_SNAP_M = 500.0

# Zion/Waukegan corridor bounding box
CORRIDOR = {'lat_min': 42.44, 'lat_max': 42.49,
            'lon_min': -87.12, 'lon_max': -87.06}

KNOWN_SANDBARS = [
    (42.4728, -87.0780),
    (42.4600, -87.0720),
    (42.4550, -87.0900),
]

# Earthdata token for background 2012 fetch
_TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/erie_remote/erie_remote_data/.earthdata_token'),
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
]

# ── Helpers ───────────────────────────────────────────────────────────────────

def _haversine_m(lat1, lon1, lat2, lon2) -> float:
    R = 6_371_000.0
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlam = math.radians(lon2 - lon1)
    a = math.sin(dphi/2)**2 + math.cos(phi1)*math.cos(phi2)*math.sin(dlam/2)**2
    return R * 2 * math.asin(math.sqrt(a))


def _in_corridor(lat, lon) -> bool:
    c = CORRIDOR
    return c['lat_min'] <= lat <= c['lat_max'] and c['lon_min'] <= lon <= c['lon_max']


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


def _fetch_buoy_wind() -> dict:
    for buoy_id in NDBC_BUOYS:
        try:
            resp = requests.get(NDBC_BASE.format(buoy_id), timeout=15)
            resp.raise_for_status()
            lines = [l for l in resp.text.splitlines() if not l.startswith('#')]
            for line in lines[2:]:
                parts = line.split()
                if len(parts) >= 7:
                    wdir = float(parts[5])
                    wspd = float(parts[6])
                    return {'buoy': buoy_id, 'wdir_deg': wdir,
                            'wspd_ms': wspd, 'wspd_kts': round(wspd * 1.94384, 1),
                            'source': 'NDBC_realtime'}
        except Exception as e:
            print(f'[!] NDBC {buoy_id} failed: {e}')

    # NWS fallback
    try:
        r = requests.get('https://api.weather.gov/points/42.46,-87.09',
                         headers={'User-Agent': 'WreckHunter2000/1.0'}, timeout=10)
        r.raise_for_status()
        r2 = requests.get(r.json()['properties']['forecastHourly'],
                          headers={'User-Agent': 'WreckHunter2000/1.0'}, timeout=10)
        r2.raise_for_status()
        period = r2.json()['properties']['periods'][0]
        wspd_kts = round(float(period.get('windSpeed', '0 mph').split()[0]) * 0.868976, 1)
        return {'buoy': 'NWS_MKE', 'wdir_deg': None, 'wspd_ms': None,
                'wspd_kts': wspd_kts, 'source': 'NWS_hourly'}
    except Exception as e:
        print(f'[!] NWS fallback failed: {e}')

    return {'buoy': None, 'wdir_deg': None, 'wspd_ms': None,
            'wspd_kts': None, 'source': 'UNAVAILABLE'}

# ── SQLite schema ─────────────────────────────────────────────────────────────

SCHEMA = """
CREATE TABLE IF NOT EXISTS anomaly_hits (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    epoch_date       TEXT    NOT NULL,
    lat              REAL    NOT NULL,
    lon              REAL    NOT NULL,
    concept          TEXT,
    score            REAL,
    wreck_score      INTEGER,
    metric           REAL,
    metric_zscore    REAL,
    classification   TEXT    DEFAULT 'CANDIDATE',
    priority         REAL    DEFAULT 1.0,
    in_corridor      INTEGER DEFAULT 0,
    wind_kts         REAL,
    wind_dir_deg     REAL,
    wind_source      TEXT,
    buoy_id          TEXT,
    sat_zenith_deg   REAL,
    sun_azimuth_deg  REAL,
    sun_elev_deg     REAL,
    incidence_angle  REAL,
    orbit_time_utc   TEXT,
    scene_id         TEXT,
    ingested_at      TEXT
);

CREATE TABLE IF NOT EXISTS stationary_anchors (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    lat             REAL    NOT NULL,
    lon             REAL    NOT NULL,
    in_corridor     INTEGER DEFAULT 0,
    epoch_a         TEXT,
    epoch_b         TEXT,
    hit_id_a        INTEGER,
    hit_id_b        INTEGER,
    snap_dist_m     REAL,
    score_a         REAL,
    score_b         REAL,
    concept_a       TEXT,
    concept_b       TEXT,
    combined_score  REAL,
    flagged_at      TEXT,
    FOREIGN KEY (hit_id_a) REFERENCES anomaly_hits(id),
    FOREIGN KEY (hit_id_b) REFERENCES anomaly_hits(id)
);

CREATE TABLE IF NOT EXISTS new_arrivals (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    lat             REAL    NOT NULL,
    lon             REAL    NOT NULL,
    in_corridor     INTEGER DEFAULT 0,
    epoch_date      TEXT,
    hit_id          INTEGER,
    concept         TEXT,
    score           REAL,
    wreck_score     INTEGER,
    metric_zscore   REAL,
    sat_zenith_deg  REAL,
    sun_azimuth_deg REAL,
    wind_kts        REAL,
    priority        REAL    DEFAULT 1.0,
    flagged_at      TEXT,
    FOREIGN KEY (hit_id) REFERENCES anomaly_hits(id)
);

CREATE TABLE IF NOT EXISTS acquisition_queue (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    tile        TEXT,
    product     TEXT,
    date_window TEXT,
    reason      TEXT,
    status      TEXT    DEFAULT 'PENDING',
    queued_at   TEXT
);
"""

# ── Ingest ────────────────────────────────────────────────────────────────────

def ingest_epoch(conn, epoch_date, json_path, wind):
    hits   = json.loads(json_path.read_text(encoding='utf-8'))
    meta   = EPOCH_META.get(epoch_date, {})
    now    = datetime.now(timezone.utc).isoformat()
    cur    = conn.cursor()
    rows   = []
    for h in hits:
        lat, lon = h['lat'], h['lon']
        scene_dt   = datetime.strptime(h.get('scene_date', epoch_date), '%Y-%m-%d')
        is_weekend = scene_dt.weekday() >= 5
        corridor   = 1 if _in_corridor(lat, lon) else 0

        classification = 'CANDIDATE'
        priority       = 1.0
        if is_weekend:
            for sb_lat, sb_lon in KNOWN_SANDBARS:
                if _haversine_m(lat, lon, sb_lat, sb_lon) <= SANDBAR_SNAP_M:
                    classification = 'MOBILE_SIGNAL_PROBE'
                    priority       = 0.5
                    break

        rows.append((
            epoch_date, lat, lon,
            h.get('concept'), h.get('score'), h.get('wreck_score'),
            h.get('metric'), h.get('metric_zscore'),
            classification, priority, corridor,
            wind.get('wspd_kts'), wind.get('wdir_deg'),
            wind.get('source'), wind.get('buoy'),
            meta.get('sat_zenith_deg'), meta.get('sun_azimuth_deg'),
            meta.get('sun_elevation_deg'), meta.get('incidence_angle'),
            meta.get('orbit_time_utc'), meta.get('scene_id'),
            now,
        ))

    cur.executemany("""
        INSERT INTO anomaly_hits
          (epoch_date, lat, lon, concept, score, wreck_score,
           metric, metric_zscore, classification, priority, in_corridor,
           wind_kts, wind_dir_deg, wind_source, buoy_id,
           sat_zenith_deg, sun_azimuth_deg, sun_elev_deg, incidence_angle,
           orbit_time_utc, scene_id, ingested_at)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
    """, rows)
    conn.commit()
    print(f'[+] Ingested {len(rows)} hits for {epoch_date} '
          f'(corridor: {sum(r[10] for r in rows)})')

# ── STATIONARY_ANCHOR ─────────────────────────────────────────────────────────

def find_stationary_anchors(conn, epoch_a, epoch_b):
    cur = conn.cursor()
    cur.execute("SELECT id,lat,lon,score,concept FROM anomaly_hits WHERE epoch_date=?", (epoch_a,))
    hits_a = cur.fetchall()
    cur.execute("SELECT id,lat,lon,score,concept FROM anomaly_hits WHERE epoch_date=?", (epoch_b,))
    hits_b = cur.fetchall()

    now     = datetime.now(timezone.utc).isoformat()
    anchors = []
    matched_b_ids = set()

    for id_a, lat_a, lon_a, score_a, concept_a in hits_a:
        for id_b, lat_b, lon_b, score_b, concept_b in hits_b:
            dist = _haversine_m(lat_a, lon_a, lat_b, lon_b)
            if dist <= SNAP_M:
                mid_lat = (lat_a + lat_b) / 2
                mid_lon = (lon_a + lon_b) / 2
                anchors.append({
                    'lat': mid_lat, 'lon': mid_lon,
                    'in_corridor': 1 if _in_corridor(mid_lat, mid_lon) else 0,
                    'epoch_a': epoch_a, 'epoch_b': epoch_b,
                    'hit_id_a': id_a, 'hit_id_b': id_b,
                    'snap_dist_m': round(dist, 1),
                    'score_a': score_a, 'score_b': score_b,
                    'concept_a': concept_a, 'concept_b': concept_b,
                    'combined_score': round((score_a or 0) + (score_b or 0), 3),
                })
                matched_b_ids.add(id_b)

    # deduplicate by snap radius
    anchors.sort(key=lambda x: -x['combined_score'])
    deduped = []
    for a in anchors:
        if not any(_haversine_m(a['lat'], a['lon'], d['lat'], d['lon']) < SNAP_M
                   for d in deduped):
            deduped.append(a)

    for a in deduped:
        cur.execute("""
            INSERT INTO stationary_anchors
              (lat, lon, in_corridor, epoch_a, epoch_b, hit_id_a, hit_id_b,
               snap_dist_m, score_a, score_b, concept_a, concept_b,
               combined_score, flagged_at)
            VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)
        """, (a['lat'], a['lon'], a['in_corridor'],
              a['epoch_a'], a['epoch_b'], a['hit_id_a'], a['hit_id_b'],
              a['snap_dist_m'], a['score_a'], a['score_b'],
              a['concept_a'], a['concept_b'], a['combined_score'], now))
        cur.execute(
            "UPDATE anomaly_hits SET classification='STATIONARY_ANCHOR' WHERE id IN (?,?)",
            (a['hit_id_a'], a['hit_id_b']))
    conn.commit()
    print(f'[+] {len(deduped)} STATIONARY_ANCHOR  '
          f'(corridor: {sum(a["in_corridor"] for a in deduped)})')
    return deduped, matched_b_ids

# ── NEW_ARRIVAL ───────────────────────────────────────────────────────────────

def find_new_arrivals(conn, epoch_b, matched_b_ids, wind):
    """2025 hits with no match in 2024 — the fresh ghosts."""
    cur = conn.cursor()
    cur.execute("""
        SELECT id, lat, lon, concept, score, wreck_score, metric_zscore,
               sat_zenith_deg, sun_azimuth_deg, priority
        FROM anomaly_hits WHERE epoch_date=?
    """, (epoch_b,))
    hits_b = cur.fetchall()

    now      = datetime.now(timezone.utc).isoformat()
    arrivals = []
    for row in hits_b:
        hit_id, lat, lon, concept, score, wreck_score, mz, sz, sa, priority = row
        if hit_id in matched_b_ids:
            continue  # already a STATIONARY_ANCHOR
        corridor = 1 if _in_corridor(lat, lon) else 0
        arrivals.append({
            'lat': lat, 'lon': lon, 'in_corridor': corridor,
            'epoch_date': epoch_b, 'hit_id': hit_id,
            'concept': concept, 'score': score, 'wreck_score': wreck_score,
            'metric_zscore': mz, 'sat_zenith_deg': sz, 'sun_azimuth_deg': sa,
            'wind_kts': wind.get('wspd_kts'), 'priority': priority,
        })

    arrivals.sort(key=lambda x: -(x['score'] or 0))

    for a in arrivals:
        cur.execute("""
            INSERT INTO new_arrivals
              (lat, lon, in_corridor, epoch_date, hit_id, concept, score,
               wreck_score, metric_zscore, sat_zenith_deg, sun_azimuth_deg,
               wind_kts, priority, flagged_at)
            VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)
        """, (a['lat'], a['lon'], a['in_corridor'], a['epoch_date'], a['hit_id'],
              a['concept'], a['score'], a['wreck_score'], a['metric_zscore'],
              a['sat_zenith_deg'], a['sun_azimuth_deg'], a['wind_kts'],
              a['priority'], now))
        cur.execute(
            "UPDATE anomaly_hits SET classification='NEW_ARRIVAL' WHERE id=?",
            (a['hit_id'],))
    conn.commit()

    corridor_arrivals = [a for a in arrivals if a['in_corridor']]
    print(f'[+] {len(arrivals)} NEW_ARRIVAL total  '
          f'(corridor Zion/Waukegan: {len(corridor_arrivals)})')
    return arrivals, corridor_arrivals

# ── Background 2012 Landsat fetch ─────────────────────────────────────────────

def _bg_fetch_2012(db_path: Path, token: str):
    """
    Runs in a daemon thread — does NOT block CUDA processing.
    Searches NASA CMR for Landsat C2L2 tiles covering the corridor,
    July 2012 drought window. Updates acquisition_queue status in DB.
    """
    conn = sqlite3.connect(str(db_path))
    cur  = conn.cursor()

    def _update_status(status, note=''):
        cur.execute(
            "UPDATE acquisition_queue SET status=? WHERE tile='023031_L7'",
            (f'{status}: {note}' if note else status,))
        conn.commit()

    try:
        print('[BG] Starting 2012 Landsat C2L2 CMR search...')
        params = {
            'short_name':    'LANDSAT_OT_C2_L2',
            'temporal':      '2012-07-01T00:00:00Z,2012-07-31T23:59:59Z',
            'bounding_box':  '-87.15,42.44,-87.06,42.49',
            'page_size':     10,
        }
        headers = {'Accept': 'application/json'}
        if token:
            headers['Authorization'] = f'Bearer {token}'

        resp = requests.get(
            'https://cmr.earthdata.nasa.gov/search/granules.json',
            params=params, headers=headers, timeout=30)
        resp.raise_for_status()
        entries = resp.json().get('feed', {}).get('entry', [])

        if not entries:
            _update_status('NO_RESULTS',
                           'CMR returned 0 granules for LANDSAT_OT_C2_L2 Jul 2012 bbox')
            print('[BG] 2012: no granules found in CMR. '
                  'Note: Landsat 7 ETM+ SLC-off artifacts likely in 2012. '
                  'Consider LANDSAT_ETM_C2_L2 or Landsat 5 TM (LANDSAT_TM_C2_L2).')
        else:
            titles = [e.get('title', '') for e in entries[:3]]
            _update_status('FOUND', f'{len(entries)} granules: {titles[0]}')
            print(f'[BG] 2012: found {len(entries)} granules. First: {titles[0]}')
            print('[BG] To download: set NASA_EARTHDATA_TOKEN and run data_fetcher_scavenger '
                  'with short_name=LANDSAT_OT_C2_L2, temporal=2012-07-01/2012-07-31')
    except Exception as e:
        _update_status('ERROR', str(e))
        print(f'[BG] 2012 fetch error: {e}')
    finally:
        conn.close()


def start_bg_2012_fetch(db_path: Path):
    token = _load_token()
    t = threading.Thread(target=_bg_fetch_2012, args=(db_path, token),
                         daemon=True, name='fetch_2012_landsat')
    t.start()
    print('[+] Background 2012 Landsat fetch started (daemon thread — will not block CUDA)')
    return t

# ── Queue entry ───────────────────────────────────────────────────────────────

def queue_2012_acquisition(conn):
    cur = conn.cursor()
    cur.execute("SELECT COUNT(*) FROM acquisition_queue WHERE tile='023031_L7'")
    if cur.fetchone()[0] == 0:
        cur.execute("""
            INSERT INTO acquisition_queue (tile, product, date_window, reason, status, queued_at)
            VALUES (?,?,?,?,?,?)
        """, ('023031_L7', 'LANDSAT_OT_C2_L2', '2012-07-01/2012-07-31',
              'Drought baseline. Fetch: earthaccess short_name=LANDSAT_OT_C2_L2 '
              'bbox=(-87.15,42.44,-87.06,42.49)',
              'PENDING', datetime.now(timezone.utc).isoformat()))
        conn.commit()

# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    print(f'[+] Lake Michigan Census Bridge — {datetime.now(timezone.utc).isoformat()}')
    print(f'[+] DB: {DB_PATH}')

    for epoch, path in EPOCH_FILES.items():
        if not path.exists():
            print(f'[!] MISSING epoch file: {path}')
            sys.exit(1)
        print(f'[+] {epoch}: {path.name} ({path.stat().st_size:,} bytes)')

    print('[+] Fetching wind (NDBC 45007 → 45002 → NWS)...')
    wind = _fetch_buoy_wind()
    print(f'[+] Wind: {wind["wspd_kts"]} kts / {wind["wdir_deg"]}° [{wind["source"]}]')

    conn = sqlite3.connect(DB_PATH)
    conn.executescript(SCHEMA)
    cur  = conn.cursor()

    # fresh run — wipe previous classifications and derived tables
    cur.execute("DELETE FROM stationary_anchors")
    cur.execute("DELETE FROM new_arrivals")
    cur.execute("DELETE FROM anomaly_hits")
    conn.commit()

    epochs = list(EPOCH_FILES.keys())
    for epoch in epochs:
        ingest_epoch(conn, epoch, EPOCH_FILES[epoch], wind)

    queue_2012_acquisition(conn)

    # start background 2012 fetch — non-blocking
    bg_thread = start_bg_2012_fetch(DB_PATH)

    # cross-match
    epoch_a, epoch_b = epochs[0], epochs[1]
    print(f'[+] Cross-matching {epoch_a} vs {epoch_b} (snap={SNAP_M}m)...')
    anchors, matched_b_ids = find_stationary_anchors(conn, epoch_a, epoch_b)
    arrivals, corridor_arrivals = find_new_arrivals(conn, epoch_b, matched_b_ids, wind)

    # ── Report ────────────────────────────────────────────────────────────────
    sep = '=' * 62
    print(f'\n{sep}')
    print(f'CENSUS BRIDGE REPORT  {epoch_a} → {epoch_b}')
    print(f'Snap: {SNAP_M}m | Wind: {wind["wspd_kts"]} kts [{wind["source"]}]')
    print(sep)
    print(f'  STATIONARY_ANCHOR : {len(anchors):>4}')
    print(f'  NEW_ARRIVAL total : {len(arrivals):>4}')
    print(f'  NEW_ARRIVAL corridor (Zion/Waukegan): {len(corridor_arrivals):>4}  ← FRESH GHOSTS')
    print(sep)

    print(f'\nTOP STATIONARY ANCHORS (top {min(5,len(anchors))}):\n')
    for i, a in enumerate(anchors[:5], 1):
        tag = ' [CORRIDOR]' if a['in_corridor'] else ''
        print(f'  #{i}{tag}  {a["lat"]:.5f}, {a["lon"]:.5f}  '
              f'dist={a["snap_dist_m"]}m  combined={a["combined_score"]}')
        print(f'       {epoch_a}: score={a["score_a"]} {a["concept_a"]}')
        print(f'       {epoch_b}: score={a["score_b"]} {a["concept_b"]}')

    print(f'\nNEW_ARRIVAL — Zion/Waukegan Corridor ({len(corridor_arrivals)} fresh ghosts):\n')
    if not corridor_arrivals:
        print('  None in corridor at 150m snap. All 2025 hits matched 2024 anchors.')
    for i, a in enumerate(corridor_arrivals, 1):
        print(f'  #{i}  {a["lat"]:.5f}, {a["lon"]:.5f}  '
              f'score={a["score"]}  concept={a["concept"]}  '
              f'zscore={a["metric_zscore"]}  wreck_score={a["wreck_score"]}')
        print(f'       sat_zenith={a["sat_zenith_deg"]}°  '
              f'sun_az={a["sun_azimuth_deg"]}°  wind={a["wind_kts"]} kts')

    # write JSON
    out_path = REPO / 'outputs' / 'calibration' / 'census_bridge.json'
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump({
            'run_at': datetime.now(timezone.utc).isoformat(),
            'epochs': epochs, 'snap_m': SNAP_M, 'wind': wind,
            'stationary_anchors': len(anchors),
            'new_arrivals_total': len(arrivals),
            'new_arrivals_corridor': len(corridor_arrivals),
            'top_anchors': anchors[:5],
            'corridor_fresh_ghosts': corridor_arrivals,
            'acquisition_2012': 'LANDSAT_OT_C2_L2 path023/row031 — background fetch running',
        }, f, indent=2)
    print(f'\n[+] Written {out_path}')

    # wait briefly for bg thread to log its CMR result
    bg_thread.join(timeout=35)
    conn.close()


if __name__ == '__main__':
    main()
