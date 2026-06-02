"""
init_wreckhunter_db.py

Initialize the WreckHunter 2026 Multi-Sensor Database

This database is designed for:
  - Multi-sensor satellite scanning (optical, SAR, thermal, SWOT, ICESat-2)
  - Multi-run target tracking (same target across different dates/conditions)
  - Confidence boosting for repeat detections
  - Tile coverage history (avoid re-scanning same areas)
  - Satellite angle metadata (sun/sat geometry for each detection)
  - Future cross-referencing with:
    - Magnetometer data (offset detection, not just spikes)
    - NOAA BAG bathymetry (including reconstructed masked areas)
    - Swayze database (historical wreck records)
    - Unredacted NOAA PDFs

Database Location: c:/Users/thomf/programming/Bagrecovery/outputs/wreckhunter_2026.db
"""

import sqlite3
from pathlib import Path
from datetime import datetime, timezone

# Database location (separate from BAG scanner)
DB_REPO = Path('c:/Users/thomf/programming/Bagrecovery/outputs')
DB_PATH = DB_REPO / 'wreckhunter_2026.db'

# Schema
SCHEMA = """
-- ── SCAN SESSIONS ────────────────────────────────────────────────────────────
-- Each scan run (lake + date + scenario combination)
CREATE TABLE IF NOT EXISTS scan_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_uuid TEXT UNIQUE NOT NULL,
    lake_region TEXT NOT NULL,
    scenario TEXT NOT NULL,
    scan_date TEXT NOT NULL,
    date_range_start TEXT,
    date_range_end TEXT,
    seasonal_window TEXT,  -- spring_post_ice, late_summer_mussel, etc.
    
    -- Satellite coverage
    sentinel2_scenes INTEGER DEFAULT 0,
    sentinel1_granules INTEGER DEFAULT 0,
    landsat_granules INTEGER DEFAULT 0,
    swot_granules INTEGER DEFAULT 0,
    icesat2_granules INTEGER DEFAULT 0,
    
    -- Processing metadata
    cuda_enabled INTEGER DEFAULT 1,
    gpu_name TEXT,
    processing_time_sec REAL,
    
    -- Output files
    kml_path TEXT,
    kmz_path TEXT,
    json_path TEXT,
    
    -- Statistics
    total_targets INTEGER DEFAULT 0,
    high_confidence INTEGER DEFAULT 0,
    medium_confidence INTEGER DEFAULT 0,
    low_confidence INTEGER DEFAULT 0,
    pending_confidence INTEGER DEFAULT 0,
    avg_score REAL,
    
    -- Status
    status TEXT DEFAULT 'COMPLETE',  -- COMPLETE, FAILED, PARTIAL
    notes TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ── TILE COVERAGE ────────────────────────────────────────────────────────────
-- Track which tiles have been scanned and when
CREATE TABLE IF NOT EXISTS tile_coverage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    lake_region TEXT NOT NULL,
    tile_id TEXT NOT NULL,  -- Sentinel-2 tile ID (e.g., "16TDN")
    
    -- Scan history
    first_scan_date TEXT,
    last_scan_date TEXT,
    scan_count INTEGER DEFAULT 1,
    
    -- Best results so far
    best_session_id INTEGER,
    best_avg_score REAL,
    best_target_count INTEGER,
    
    -- Seasonal coverage
    scanned_spring INTEGER DEFAULT 0,
    scanned_summer INTEGER DEFAULT 0,
    scanned_fall INTEGER DEFAULT 0,
    scanned_winter INTEGER DEFAULT 0,
    
    -- Sensor coverage
    has_optical INTEGER DEFAULT 0,
    has_sar INTEGER DEFAULT 0,
    has_thermal INTEGER DEFAULT 0,
    has_swot INTEGER DEFAULT 0,
    has_icesat2 INTEGER DEFAULT 0,
    has_mag INTEGER DEFAULT 0,
    has_bag INTEGER DEFAULT 0,
    
    UNIQUE(lake_region, tile_id),
    FOREIGN KEY (best_session_id) REFERENCES scan_sessions(id)
);

-- ── TARGETS ──────────────────────────────────────────────────────────────────
-- Individual anomaly detections
CREATE TABLE IF NOT EXISTS targets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_uuid TEXT UNIQUE NOT NULL,
    
    -- Session linkage
    session_id INTEGER NOT NULL,
    
    -- Location (WGS84)
    lat REAL NOT NULL,
    lon REAL NOT NULL,
    depth_m REAL,  -- If known from bathymetry
    
    -- Detection metadata
    concept TEXT,  -- shadow_roughness, zebra_clarity, etc.
    score REAL NOT NULL,
    wreck_score INTEGER,
    zscore REAL,
    metric REAL,
    metric_zscore REAL,
    
    -- Confidence level
    confidence TEXT DEFAULT 'PENDING',  -- HIGH, MEDIUM, LOW, PENDING
    confidence_score REAL,  -- Normalized 0-1 confidence
    
    -- Satellite angles (for this detection)
    sun_azimuth REAL,
    sun_elevation REAL,
    sat_zenith REAL,
    incidence_angle REAL,
    scene_id TEXT,
    epoch_date TEXT,
    
    -- Sensor-specific data
    sensor_type TEXT,  -- optical, sar, thermal, swot, icesat2, mag, bag
    sensor_data TEXT,  -- JSON blob with sensor-specific fields
    
    -- Multi-run tracking
    is_repeat_detection INTEGER DEFAULT 0,
    parent_target_id INTEGER,  -- Links to first detection of this target
    repeat_count INTEGER DEFAULT 1,  -- How many times this target has been seen
    
    -- Cross-reference IDs
    mag_anomaly_id INTEGER,  -- Links to magnetometer database
    bag_survey_id INTEGER,   -- Links to BAG bathymetry
    swayze_id INTEGER,       -- Links to Swayze historical database
    pdf_doc_id INTEGER,      -- Links to unredacted PDF database
    
    -- Status
    verified INTEGER DEFAULT 0,  -- Manually verified as real target
    false_positive INTEGER DEFAULT 0,
    notes TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (session_id) REFERENCES scan_sessions(id),
    FOREIGN KEY (parent_target_id) REFERENCES targets(id)
);

-- ── TARGET CLUSTERS ─────────────────────────────────────────────────────────
-- Group multiple detections of the same physical target
CREATE TABLE IF NOT EXISTS target_clusters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_uuid TEXT UNIQUE NOT NULL,
    
    -- Cluster center
    center_lat REAL NOT NULL,
    center_lon REAL NOT NULL,
    radius_m REAL DEFAULT 50.0,  -- Cluster radius
    
    -- Member targets
    member_count INTEGER DEFAULT 1,
    member_target_ids TEXT,  -- JSON array of target IDs
    
    -- Aggregated confidence
    avg_score REAL,
    max_score REAL,
    confidence_level TEXT,  -- Upgraded based on repeat detections
    
    -- Multi-sensor agreement
    sensors_detected TEXT,  -- JSON array: ["optical", "sar", "thermal"]
    sensor_agreement_score REAL,  -- 0-1 based on sensor consensus
    
    -- Temporal analysis
    first_seen_date TEXT,
    last_seen_date TEXT,
    temporal_stability REAL,  -- How consistent across time
    
    -- Classification
    classification TEXT,  -- wreck, geology, false_positive, unknown
    classification_confidence REAL,
    
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ── MAGNETOMETER ANOMALIES ──────────────────────────────────────────────────
-- For mag scanner integration (offset detection, not just spikes)
CREATE TABLE IF NOT EXISTS mag_anomalies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    anomaly_uuid TEXT UNIQUE NOT NULL,
    
    -- Location
    lat REAL NOT NULL,
    lon REAL NOT NULL,
    depth_m REAL,
    
    -- Magnetic signature
    field_offset_nt REAL,  -- Nanoteslas offset from background
    field_spike_nt REAL,   -- Peak spike value
    anomaly_length_m REAL,
    anomaly_width_m REAL,
    
    -- Signature type
    signature_type TEXT,  -- dipole, monopole, complex
    polarity TEXT,  -- normal, reversed, mixed
    
    -- Processing
    background_model TEXT,
    filter_applied TEXT,
    snr REAL,  -- Signal-to-noise ratio
    
    -- Cross-reference
    linked_target_id INTEGER,  -- Links to targets table
    bag_survey_id INTEGER,
    
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (linked_target_id) REFERENCES targets(id)
);

-- ── BAG BATHYMETRY ──────────────────────────────────────────────────────────
-- For NOAA BAG file integration (including reconstructed masked areas)
CREATE TABLE IF NOT EXISTS bag_surveys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    survey_uuid TEXT UNIQUE NOT NULL,
    
    -- Survey metadata
    survey_id TEXT,  -- NOAA survey ID (e.g., "H12133")
    area_name TEXT,
    
    -- Coverage
    bbox_w REAL,
    bbox_s REAL,
    bbox_e REAL,
    bbox_n REAL,
    resolution_m REAL,
    
    -- Data quality
    has_masked_areas INTEGER DEFAULT 0,
    masked_area_pct REAL,
    reconstruction_attempted INTEGER DEFAULT 0,
    reconstruction_quality TEXT,  -- none, low, medium, high
    
    -- Uncertainty analysis
    avg_uncertainty_m REAL,
    max_uncertainty_m REAL,
    
    -- Cross-reference
    linked_target_ids TEXT,  -- JSON array of target IDs
    linked_mag_anomaly_ids TEXT,  -- JSON array
    
    -- File paths
    bag_file_path TEXT,
    uncertainty_file_path TEXT,
    reconstructed_model_path TEXT,  -- 3D model of masked areas
    
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ── NOAA PDF DOCUMENTS ──────────────────────────────────────────────────────
-- For unredacted PDF integration
CREATE TABLE IF NOT EXISTS noaa_pdfs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_uuid TEXT UNIQUE NOT NULL,
    
    -- Document metadata
    document_id TEXT,  -- NOAA document ID
    title TEXT,
    publication_date TEXT,
    
    -- Redaction analysis
    redaction_count INTEGER DEFAULT 0,
    redaction_method TEXT,  -- black_box, text_removal, image_mask
    unredaction_attempted INTEGER DEFAULT 0,
    unredaction_success INTEGER DEFAULT 0,
    unredacted_content TEXT,  -- Recovered text/data
    
    -- Geographic references
    mentioned_locations TEXT,  -- JSON array of lat/lon or place names
    mentioned_surveys TEXT,  -- JSON array of survey IDs
    
    -- Cross-reference
    linked_target_ids TEXT,
    linked_bag_survey_ids TEXT,
    
    -- File paths
    original_pdf_path TEXT,
    unredacted_pdf_path TEXT,
    
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ── SWAYZE DATABASE LINKS ───────────────────────────────────────────────────
-- Historical wreck records
CREATE TABLE IF NOT EXISTS swayze_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    record_uuid TEXT UNIQUE NOT NULL,
    
    -- Wreck metadata
    vessel_name TEXT,
    vessel_type TEXT,
    year_lost INTEGER,
    cause_of_loss TEXT,
    
    -- Location (historical, may be approximate)
    reported_lat REAL,
    reported_lon REAL,
    location_accuracy TEXT,  -- exact, approximate, unknown
    
    -- Historical sources
    source_documents TEXT,  -- JSON array of references
    
    -- Cross-reference
    linked_target_ids TEXT,
    linked_mag_anomaly_ids TEXT,
    linked_bag_survey_ids TEXT,
    
    -- Verification
    verified_location INTEGER DEFAULT 0,
    verification_method TEXT,
    
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ── INDEXES ─────────────────────────────────────────────────────────────────

CREATE INDEX IF NOT EXISTS idx_targets_location ON targets(lat, lon);
CREATE INDEX IF NOT EXISTS idx_targets_session ON targets(session_id);
CREATE INDEX IF NOT EXISTS idx_targets_confidence ON targets(confidence);
CREATE INDEX IF NOT EXISTS idx_targets_repeat ON targets(is_repeat_detection, parent_target_id);
CREATE INDEX IF NOT EXISTS idx_targets_cross_ref ON targets(mag_anomaly_id, bag_survey_id, swayze_id);

CREATE INDEX IF NOT EXISTS idx_tile_coverage_location ON tile_coverage(lake_region, tile_id);

CREATE INDEX IF NOT EXISTS idx_mag_anomalies_location ON mag_anomalies(lat, lon);
CREATE INDEX IF NOT EXISTS idx_mag_anomalies_linked ON mag_anomalies(linked_target_id);

CREATE INDEX IF NOT EXISTS idx_bag_surveys_location ON bag_surveys(bbox_w, bbox_s, bbox_e, bbox_n);

CREATE INDEX IF NOT EXISTS idx_clusters_location ON target_clusters(center_lat, center_lon);

-- ── VIEWS ───────────────────────────────────────────────────────────────────

-- High-confidence targets (repeat detections or multi-sensor agreement)
CREATE VIEW IF NOT EXISTS high_confidence_targets AS
SELECT t.*, tc.cluster_uuid, tc.sensor_agreement_score, tc.temporal_stability
FROM targets t
LEFT JOIN target_clusters tc ON t.id IN (SELECT json_each.value FROM json_each(tc.member_target_ids))
WHERE t.confidence = 'HIGH'
   OR t.repeat_count >= 2
   OR tc.sensor_agreement_score >= 0.8;

-- Targets needing verification
CREATE VIEW IF NOT EXISTS targets_needing_verification AS
SELECT t.*, ts.session_id, ts.lake_region, ts.scenario
FROM targets t
JOIN scan_sessions ts ON t.session_id = ts.id
WHERE t.verified = 0
  AND t.false_positive = 0
  AND t.confidence IN ('HIGH', 'MEDIUM')
ORDER BY t.score DESC, t.repeat_count DESC;

-- Multi-sensor targets (detected by 2+ sensor types)
CREATE VIEW IF NOT EXISTS multi_sensor_targets AS
SELECT tc.*, json_array_length(tc.sensors_detected) as sensor_count
FROM target_clusters tc
WHERE json_array_length(tc.sensors_detected) >= 2;

"""

def init_database():
    """Initialize the WreckHunter 2026 database."""
    print(f'[+] Initializing WreckHunter 2026 Database')
    print(f'    Location: {DB_PATH}')
    
    # Create database directory if needed
    DB_REPO.mkdir(parents=True, exist_ok=True)
    
    # Connect and create schema
    conn = sqlite3.connect(str(DB_PATH))
    conn.executescript(SCHEMA)
    conn.commit()
    
    # Verify tables
    cur = conn.cursor()
    cur.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    tables = [row[0] for row in cur.fetchall()]
    
    print(f'[+] Created {len(tables)} tables:')
    for table in tables:
        print(f'    - {table}')
    
    # Verify views
    cur.execute("SELECT name FROM sqlite_master WHERE type='view' ORDER BY name")
    views = [row[0] for row in cur.fetchall()]
    
    print(f'[+] Created {len(views)} views:')
    for view in views:
        print(f'    - {view}')
    
    conn.close()
    
    print()
    print('=' * 62)
    print('DATABASE READY')
    print('=' * 62)
    print(f'  Location: {DB_PATH}')
    print(f'  Tables: {len(tables)}')
    print(f'  Views: {len(views)}')
    print()
    print('  Ready for:')
    print('    - Multi-sensor satellite scanning')
    print('    - Multi-run target tracking')
    print('    - Magnetometer integration')
    print('    - NOAA BAG bathymetry (including masked reconstruction)')
    print('    - NOAA PDF unredaction')
    print('    - Swayze database cross-referencing')
    print('=' * 62)
    
    return DB_PATH

if __name__ == '__main__':
    init_database()
