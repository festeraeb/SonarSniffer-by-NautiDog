"""
init_cesarops_db.py

Initialize consolidated CESAROPS databases.
Creates unified schema in wreckhunter2000/databases/cesarops/

Marks old fragmented databases as OFF LIMITS.
"""

import sqlite3
from pathlib import Path
from datetime import datetime

# ── Database Paths ───────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
DB_DIR = REPO / 'databases' / 'cesarops'
DB_DIR.mkdir(parents=True, exist_ok=True)

# New consolidated databases
NEW_DBS = {
    'drift_simulations.db': 'OpenDrift simulation results and trajectories',
    'search_areas.db': 'SAR search area definitions and priorities',
    'assets.db': 'Air/sea assets for CESAROPS coordination',
    'incidents.db': 'Incident/SAR case tracking',
}

# Old databases to mark as OFF LIMITS
OLD_DB_PATTERNS = [
    'Bagrecovery/db/cesarops*.db',
    'Bagrecovery/db/drift_objects*.db',
    'Bagrecovery/db/great_lakes*.db',
    'cesarops*/drift_objects.db',
]

# ── Schema Definitions ──────────────────────────────────────────────────────

DRIFT_SIMULATIONS_SCHEMA = """
-- Drift simulation results from OpenDrift
CREATE TABLE IF NOT EXISTS simulations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    simulation_uuid TEXT UNIQUE NOT NULL,
    
    -- Parameters
    lake TEXT NOT NULL,
    seed_lat REAL NOT NULL,
    seed_lon REAL NOT NULL,
    seed_time TEXT NOT NULL,
    duration_hours INTEGER NOT NULL,
    num_particles INTEGER DEFAULT 100,
    
    -- Results
    status TEXT DEFAULT 'pending',  -- pending, running, complete, failed
    start_time TEXT,
    end_time TEXT,
    final_positions_json TEXT,  -- JSON array of [lat, lon] pairs
    trajectory_json TEXT,  -- Full trajectory data
    
    -- Metadata
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    created_by TEXT,
    notes TEXT,
    
    -- Foreign keys
    incident_id INTEGER,  -- Links to incidents table
    search_area_id INTEGER,  -- Links to search_areas table
    FOREIGN KEY (incident_id) REFERENCES incidents(id),
    FOREIGN KEY (search_area_id) REFERENCES search_areas(id)
);

-- Index for fast lookups
CREATE INDEX IF NOT EXISTS idx_simulations_lake ON simulations(lake);
CREATE INDEX IF NOT EXISTS idx_simulations_incident ON simulations(incident_id);
"""

SEARCH_AREAS_SCHEMA = """
-- SAR search area definitions
CREATE TABLE IF NOT EXISTS search_areas (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    area_uuid TEXT UNIQUE NOT NULL,
    
    -- Location
    lake TEXT NOT NULL,
    center_lat REAL NOT NULL,
    center_lon REAL NOT NULL,
    radius_nm REAL NOT NULL,  -- Radius in nautical miles
    
    -- Search parameters
    priority TEXT DEFAULT 'medium',  -- low, medium, high, critical
    search_type TEXT,  -- drift, last_known, probability
    confidence_level REAL,  -- 0-1 confidence in area
    
    -- Status
    status TEXT DEFAULT 'planned',  -- planned, active, complete, archived
    assigned_assets TEXT,  -- JSON array of asset IDs
    
    -- Metadata
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT,
    created_by TEXT,
    notes TEXT,
    
    -- Foreign keys
    incident_id INTEGER,
    FOREIGN KEY (incident_id) REFERENCES incidents(id)
);

-- Index for spatial queries
CREATE INDEX IF NOT EXISTS idx_search_areas_lake ON search_areas(lake);
CREATE INDEX IF NOT EXISTS idx_search_areas_incident ON search_areas(incident_id);
"""

ASSETS_SCHEMA = """
-- Air/sea assets for SAR coordination
CREATE TABLE IF NOT EXISTS assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id TEXT UNIQUE NOT NULL,  -- e.g., "CG-1234", "N12345"
    
    -- Asset details
    name TEXT NOT NULL,
    asset_type TEXT NOT NULL,  -- helicopter, boat, aircraft, uav, diver
    operator TEXT,  -- USCG, local PD, volunteer, etc.
    
    -- Capabilities
    max_range_nm REAL,  -- Maximum range in nautical miles
    endurance_hours REAL,  -- Maximum endurance in hours
    search_capability TEXT,  -- Visual, radar, sonar, thermal
    lakes_covered TEXT,  -- JSON array of lake names
    
    -- Status
    status TEXT DEFAULT 'available',  -- available, deployed, maintenance, offline
    current_lat REAL,
    current_lon REAL,
    last_updated TEXT,
    
    -- Contact
    contact_name TEXT,
    contact_radio TEXT,
    contact_phone TEXT,
    
    -- Metadata
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT
);

-- Index for fast lookups
CREATE INDEX IF NOT EXISTS idx_assets_type ON assets(asset_type);
CREATE INDEX IF NOT EXISTS idx_assets_status ON assets(status);
CREATE INDEX IF NOT EXISTS idx_assets_lake ON assets(lakes_covered);
"""

INCIDENTS_SCHEMA = """
-- SAR incident tracking
CREATE TABLE IF NOT EXISTS incidents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    incident_id TEXT UNIQUE NOT NULL,  -- e.g., "2025-08-22-ROSSA"
    
    -- Incident details
    title TEXT NOT NULL,
    incident_type TEXT NOT NULL,  -- vessel_sinking, missing_person, aircraft
    severity TEXT DEFAULT 'medium',  -- low, medium, high, critical
    
    -- Location
    lake TEXT NOT NULL,
    last_known_lat REAL,
    last_known_lon REAL,
    last_known_time TEXT,
    
    -- Timeline
    reported_at TEXT,
    incident_date TEXT,
    status TEXT DEFAULT 'active',  -- active, suspended, resolved, archived
    
    -- Vessel/Person details
    vessel_name TEXT,
    vessel_type TEXT,
    vessel_length_ft REAL,
    vessel_color TEXT,
    persons_aboard INTEGER,
    
    -- Metadata
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT,
    created_by TEXT,
    notes TEXT,
    
    -- Related data
    search_areas_count INTEGER DEFAULT 0,
    simulations_count INTEGER DEFAULT 0
);

-- Index for fast lookups
CREATE INDEX IF NOT EXISTS idx_incidents_status ON incidents(status);
CREATE INDEX IF NOT EXISTS idx_incidents_lake ON incidents(lake);
CREATE INDEX IF NOT EXISTS idx_incidents_date ON incidents(incident_date);
"""

# ── Initialization ───────────────────────────────────────────────────────────

def create_database(db_path: Path, schema: str, description: str):
    """Create a single database with given schema."""
    print(f'Creating {db_path.name}...')
    print(f'  Description: {description}')
    
    conn = sqlite3.connect(str(db_path))
    conn.executescript(schema)
    
    # Add metadata table
    conn.execute("""
        CREATE TABLE IF NOT EXISTS _metadata (
            key TEXT PRIMARY KEY,
            value TEXT
        )
    """)
    
    conn.execute("""
        INSERT OR REPLACE INTO _metadata (key, value) VALUES (?, ?)
    """, ('created_at', datetime.now().isoformat()))
    
    conn.execute("""
        INSERT OR REPLACE INTO _metadata (key, value) VALUES (?, ?)
    """, ('schema_version', '1.0'))
    
    conn.commit()
    conn.close()
    
    print(f'  ✓ Created successfully')
    print()


def mark_old_databases():
    """Create README marking old databases as OFF LIMITS."""
    print('Marking old fragmented databases as OFF LIMITS...')
    
    readme_path = DB_DIR.parent / 'OLD_CESAROPS_DBS_README.txt'
    
    with open(readme_path, 'w') as f:
        f.write("""
================================================================================
OLD CESAROPS DATABASES - DO NOT USE
================================================================================

These databases are FRAGMENTED and OUTDATED. Do NOT use them for new development.

CONSOLIDATED REPLACEMENT:
  wreckhunter2000/databases/cesarops/
    ├── drift_simulations.db
    ├── search_areas.db
    ├── assets.db
    └── incidents.db

OFF LIMITS DATABASES:
  - Bagrecovery/db/cesarops*.db
  - Bagrecovery/db/drift_objects*.db
  - Bagrecovery/db/great_lakes*.db
  - cesarops*/drift_objects.db
  - Any CodeChunks*.db, SemanticSymbols*.db (IDE cache files)

MIGRATION:
  Useful data from old databases should be migrated to the new consolidated
  schema. Do NOT directly reference old databases in new code.

Created: """ + datetime.now().isoformat() + """
================================================================================
""")
    
    print(f'  ✓ Created {readme_path.name}')
    print()


def main():
    """Initialize all CESAROPS databases."""
    print('='*70)
    print('CESAROPS DATABASE INITIALIZATION')
    print('='*70)
    print()
    
    # Create new consolidated databases
    for db_name, description in NEW_DBS.items():
        db_path = DB_DIR / db_name
        schema = None
        
        if 'drift_simulations' in db_name:
            schema = DRIFT_SIMULATIONS_SCHEMA
        elif 'search_areas' in db_name:
            schema = SEARCH_AREAS_SCHEMA
        elif 'assets' in db_name:
            schema = ASSETS_SCHEMA
        elif 'incidents' in db_name:
            schema = INCIDENTS_SCHEMA
        
        if schema:
            create_database(db_path, schema, description)
    
    # Mark old databases as off limits
    mark_old_databases()
    
    print('='*70)
    print('DATABASE INITIALIZATION COMPLETE')
    print('='*70)
    print()
    print(f'New databases created in: {DB_DIR}')
    print('Old databases marked as OFF LIMITS')
    print()
    print('Next: Phase 2 - Python Core Refactor')
    print('='*70)


if __name__ == '__main__':
    main()
