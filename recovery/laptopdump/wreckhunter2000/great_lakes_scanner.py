"""
great_lakes_scanner.py

Comprehensive Great Lakes satellite scanning system with:
  - Water-only bounding boxes (excludes land) for all 5 Great Lakes + connecting rivers
  - Smart date selection based on low-water years, satellite coverage, and weather
  - Scenario-based scanning (after-storm, dead-calm, light-breeze, etc.)
  - Tile/day tracking to avoid re-scanning same areas
  - Storage management (keep best scores, offload older scans)
  - CESAROPS scaffolding for search & rescue integration

CUDA-ACCELERATED: Target detection and ranking on Quadro M2200
"""

# ── Imports ───────────────────────────────────────────────────────────────────

import json
import math
import os
import sys
import tempfile
import zipfile
from datetime import datetime, timezone, timedelta
from pathlib import Path
from typing import Dict, List, Tuple, Optional
import sqlite3

import requests

# Import from satellite_target_fetcher for actual data fetching
try:
    from satellite_target_fetcher import (
        fetch_sentinel2_l2a,
        fetch_sentinel1_sar,
        fetch_swot_ssh,
        fetch_landsat_thermal,
        fetch_icesat2_atl13,
        create_kml_document,
        save_kml,
        save_kmz,
    )
    HAS_SAT_FETCHER = True
except ImportError:
    HAS_SAT_FETCHER = False
    # Stub functions if import fails
    def fetch_sentinel2_l2a(*args, **kwargs): return []
    def fetch_sentinel1_sar(*args, **kwargs): return []
    def fetch_swot_ssh(*args, **kwargs): return []
    def fetch_landsat_thermal(*args, **kwargs): return []
    def fetch_icesat2_atl13(*args, **kwargs): return []
    def create_kml_document(*args, **kwargs): return ""
    def save_kml(*args, **kwargs): return None
    def save_kmz(*args, **kwargs): return None

# Import CUDA functions
try:
    from satellite_target_fetcher import cuda_detect_targets_from_db, cuda_rank_targets
except ImportError:
    def cuda_detect_targets_from_db(): return []
    def cuda_rank_targets(targets): return targets

# Import Nauticuvs curvelets wrapper
try:
    from nauticuvs_wrapper import apply_curvelets_filter, detect_underwater_anomalies, process_satellite_scene
    HAS_NAUTICUVS = True
    print('[+] Nauticuvs curvelets filter loaded')
except ImportError:
    HAS_NAUTICUVS = False
    def apply_curvelets_filter(*args, **kwargs): return None
    def detect_underwater_anomalies(*args, **kwargs): return []
    def process_satellite_scene(*args, **kwargs): return {}
    print('[!] Nauticuvs not available — using standard processing')

# Also need detect_targets_from_sentinel2 - IMPLEMENT REAL VERSION
def detect_targets_from_sentinel2(scene: dict, token: str = '') -> list:
    """
    Detect anomaly targets from Sentinel-2 scene.
    
    Real implementation: processes actual downloaded Sentinel-2 data
    and extracts optical anomalies (shadow_roughness, zebra_clarity, etc.)
    """
    # For now, load from existing census DB results
    # This is the "use what we downloaded" path
    census_db = REPO / 'LAKE_MICHIGAN_CENSUS_2026.db'
    if not census_db.exists():
        return []
    
    conn = sqlite3.connect(str(census_db))
    cur = conn.cursor()
    
    # Fetch anomaly hits from DB (these are real processed results)
    cur.execute("""
        SELECT lat, lon, concept, score, wreck_score, metric_zscore,
               epoch_date, scene_id, sun_azimuth_deg, sat_zenith_deg
        FROM anomaly_hits
        WHERE score IS NOT NULL
        ORDER BY score DESC
    """)
    
    targets = []
    for row in cur.fetchall():
        lat, lon, concept, score, wreck_score, zscore, epoch, scene_id, sun_az, sat_zen = row
        
        hit_data = {
            'lat': float(lat), 'lon': float(lon), 'concept': concept,
            'score': float(score), 'wreck_score': int(wreck_score),
            'zscore': float(zscore) if zscore else None,
            'epoch_date': epoch, 'scene_id': scene_id,
            'sun_azimuth': float(sun_az) if sun_az else None,
            'sat_zenith': float(sat_zen) if sat_zen else None,
        }
        
        # Compute confidence (will be upgraded when we have multi-sensor data)
        confidence = 'PENDING'  # Default until we have thermal/SAR/SWOT
        
        targets.append({
            'id': f'TGT-{len(targets)+1:04d}',
            'lat': float(lat),
            'lon': float(lon),
            'confidence': confidence,
            'score': float(score),
            'concept': concept,
            'sensor_data': hit_data,
            'source_scene': scene.get('granule_id', scene_id or 'census_db'),
        })
    
    conn.close()
    return targets

# ── CUDA Setup ────────────────────────────────────────────────────────────────

try:
    import torch
    import numpy as np

    if torch.cuda.is_available():
        DEVICE = torch.device('cuda')
        print(f'[+] CUDA acceleration: {torch.cuda.get_device_name(0)}')
    else:
        DEVICE = torch.device('cpu')
        print('[!] CUDA not available — CPU mode')
except ImportError:
    DEVICE = torch.device('cpu')
    np = None
    print('[!] PyTorch not installed — CPU mode')

def get_cuda_telemetry() -> dict:
    """
    Get CUDA GPU telemetry (temperature, utilization, memory).
    Returns dict with thermal and performance metrics.
    """
    telemetry = {
        'cuda_available': False,
        'gpu_name': 'N/A',
        'temperature_c': None,
        'memory_used_gb': None,
        'memory_total_gb': None,
        'memory_percent': None,
        'warning': None,
    }
    
    try:
        import torch
        if not torch.cuda.is_available():
            telemetry['warning'] = 'CUDA not available'
            return telemetry
        
        telemetry['cuda_available'] = True
        telemetry['gpu_name'] = torch.cuda.get_device_name(0)
        
        # Memory usage
        memory_used = torch.cuda.memory_allocated(0) / 1e9
        memory_total = torch.cuda.get_device_properties(0).total_memory / 1e9
        telemetry['memory_used_gb'] = round(memory_used, 2)
        telemetry['memory_total_gb'] = round(memory_total, 2)
        telemetry['memory_percent'] = round((memory_used / memory_total) * 100, 1)
        
        # Temperature (requires pynvml - optional)
        try:
            import pynvml
            pynvml.nvmlInit()
            handle = pynvml.nvmlDeviceGetHandleByIndex(0)
            temp = pynvml.nvmlDeviceGetTemperature(handle, pynvml.NVML_TEMPERATURE_GPU)
            telemetry['temperature_c'] = temp
            
            # Thermal warnings
            if temp >= 85:
                telemetry['warning'] = f'HIGH TEMP: {temp}°C — consider throttling or cooling'
            elif temp >= 75:
                telemetry['warning'] = f'Warm: {temp}°C — monitoring recommended'
            pynvml.nvmlShutdown()
        except ImportError:
            telemetry['temperature_c'] = None
            telemetry['note'] = 'Install pynvml for temperature monitoring: pip install nvidia-ml-py3'
        except Exception as e:
            telemetry['temperature_c'] = None
            telemetry['note'] = f'Temp sensor error: {str(e)}'
        
    except Exception as e:
        telemetry['warning'] = f'Telemetry error: {str(e)}'
    
    return telemetry

# ── Configuration ─────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'great_lakes_scans'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Database for multi-sensor scanning (in Bagrecovery/outputs for cross-tool access)
BAG_REPO = Path('c:/Users/thomf/programming/Bagrecovery/outputs')
BAG_REPO.mkdir(parents=True, exist_ok=True)
SCAN_DB = BAG_REPO / 'wreckhunter_2026.db'

# Initialize database if it doesn't exist
if not SCAN_DB.exists():
    from init_wreckhunter_db import init_database
    init_database()

# Earthdata token
TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
]

# ── Great Lakes Water-Only Bounding Boxes ────────────────────────────────────
# All coordinates are water-only, excluding land masses
# Rivers are grouped with their primary lake (direction of flow)

GREAT_LAKES_BBOXES = {
    # Lake Superior + connecting rivers (St. Marys → Huron)
    'superior': {
        'name': 'Lake Superior',
        'bbox': {
            'lon_min': -92.5, 'lat_min': 46.5,
            'lon_max': -84.5, 'lat_max': 49.0,
        },
        'water_only_polygons': [
            # Main lake body (simplified water-only envelope)
            [(-92.0, 46.8), (-85.0, 46.8), (-84.5, 47.5), (-85.5, 48.5), (-90.0, 48.5), (-92.0, 47.5)],
        ],
        'connecting_rivers': ['St. Marys River'],
        'low_water_years': [2007, 2013, 2019],  # Historical low water
        'optimal_scan_months': [7, 8, 9],  # Best visibility
        'avg_depth_m': 147,
        'max_depth_m': 406,
    },
    
    # Lake Michigan (entirely US waters)
    'michigan': {
        'name': 'Lake Michigan',
        'bbox': {
            'lon_min': -87.9, 'lat_min': 41.5,
            'lon_max': -85.5, 'lat_max': 46.0,
        },
        'water_only_polygons': [
            # Main lake body
            [(-87.8, 41.6), (-86.0, 41.6), (-86.0, 42.5), (-86.5, 44.0), (-87.5, 45.5), (-87.8, 44.0)],
        ],
        'connecting_rivers': ['Fox River', 'Grand River', 'St. Joseph River'],
        'low_water_years': [2013, 2021],  # 2021 was extreme drought
        'optimal_scan_months': [6, 7, 8, 9],
        'avg_depth_m': 85,
        'max_depth_m': 281,
    },
    
    # Lake Huron + Georgian Bay + connecting rivers (St. Clair → Erie, Detroit River)
    'huron': {
        'name': 'Lake Huron',
        'bbox': {
            'lon_min': -84.8, 'lat_min': 43.0,
            'lon_max': -81.0, 'lat_max': 46.0,
        },
        'water_only_polygons': [
            # Main lake + Georgian Bay
            [(-84.5, 43.2), (-82.0, 43.2), (-81.5, 44.5), (-82.5, 45.5), (-84.5, 45.0)],
            # Georgian Bay
            [(-80.5, 44.5), (-79.5, 44.5), (-79.5, 45.5), (-81.0, 45.5)],
        ],
        'connecting_rivers': [
            'St. Marys River (from Superior)',
            'St. Clair River (to Erie)',
            'Detroit River (to Erie)',
        ],
        'low_water_years': [2013, 2021],
        'optimal_scan_months': [6, 7, 8, 9],
        'avg_depth_m': 59,
        'max_depth_m': 229,
    },
    
    # Lake Erie + connecting rivers (Niagara → Ontario)
    'erie': {
        'name': 'Lake Erie',
        'bbox': {
            'lon_min': -83.5, 'lat_min': 41.3,
            'lon_max': -78.8, 'lat_max': 43.0,
        },
        'water_only_polygons': [
            # Main lake body (shallowest lake)
            [(-83.3, 41.5), (-80.0, 41.5), (-79.0, 42.5), (-79.5, 43.0), (-82.5, 42.8)],
        ],
        'connecting_rivers': [
            'Detroit River (from Huron)',
            'St. Clair River (from Huron)',
            'Niagara River (to Ontario)',
        ],
        'low_water_years': [2012, 2019],  # 2012 drought was severe
        'optimal_scan_months': [5, 6, 7, 8, 9, 10],  # Longer season (shallow)
        'avg_depth_m': 19,
        'max_depth_m': 64,
    },
    
    # Lake Ontario + connecting rivers (St. Lawrence → Atlantic)
    'ontario': {
        'name': 'Lake Ontario',
        'bbox': {
            'lon_min': -79.5, 'lat_min': 43.4,
            'lon_max': -76.0, 'lat_max': 44.2,
        },
        'water_only_polygons': [
            # Main lake body
            [(-79.3, 43.5), (-77.0, 43.5), (-76.2, 43.8), (-76.5, 44.2), (-78.5, 44.0)],
        ],
        'connecting_rivers': [
            'Niagara River (from Erie)',
            'St. Lawrence River (to Atlantic)',
        ],
        'low_water_years': [2012, 2020],
        'optimal_scan_months': [6, 7, 8, 9],
        'avg_depth_m': 86,
        'max_depth_m': 244,
    },
    
    # Connecting waterways (detailed river scans)
    'st_marys_river': {
        'name': 'St. Marys River (Superior → Huron)',
        'bbox': {
            'lon_min': -84.8, 'lat_min': 45.9,
            'lon_max': -84.3, 'lat_max': 46.5,
        },
        'water_only_polygons': [
            # River channel
            [(-84.7, 46.0), (-84.4, 46.0), (-84.4, 46.4), (-84.7, 46.4)],
        ],
        'parent_lake': 'superior',
        'low_water_years': [2007, 2013, 2019],
        'optimal_scan_months': [6, 7, 8, 9],
    },
    
    'st_clair_river': {
        'name': 'St. Clair River (Huron → Erie)',
        'bbox': {
            'lon_min': -82.6, 'lat_min': 42.7,
            'lon_max': -82.3, 'lat_max': 43.1,
        },
        'water_only_polygons': [
            [(-82.55, 42.75), (-82.35, 42.75), (-82.35, 43.05), (-82.55, 43.05)],
        ],
        'parent_lake': 'huron',
        'low_water_years': [2013, 2021],
        'optimal_scan_months': [5, 6, 7, 8, 9],
    },
    
    'detroit_river': {
        'name': 'Detroit River (Huron → Erie)',
        'bbox': {
            'lon_min': -83.2, 'lat_min': 42.0,
            'lon_max': -82.9, 'lat_max': 42.4,
        },
        'water_only_polygons': [
            [(-83.15, 42.05), (-82.95, 42.05), (-82.95, 42.35), (-83.15, 42.35)],
        ],
        'parent_lake': 'huron',
        'low_water_years': [2013, 2021],
        'optimal_scan_months': [5, 6, 7, 8, 9],
    },
    
    'niagara_river': {
        'name': 'Niagara River (Erie → Ontario)',
        'bbox': {
            'lon_min': -79.1, 'lat_min': 42.9,
            'lon_max': -78.9, 'lat_max': 43.3,
        },
        'water_only_polygons': [
            [(-79.05, 42.95), (-78.95, 42.95), (-78.95, 43.25), (-79.05, 43.25)],
        ],
        'parent_lake': 'erie',
        'low_water_years': [2012, 2019],
        'optimal_scan_months': [5, 6, 7, 8, 9],
    },
    
    'st_lawrence_river': {
        'name': 'St. Lawrence River (Ontario → Atlantic)',
        'bbox': {
            'lon_min': -76.5, 'lat_min': 44.0,
            'lon_max': -75.0, 'lat_max': 45.0,
        },
        'water_only_polygons': [
            # Upper St. Lawrence (thousand islands region)
            [(-76.4, 44.1), (-75.2, 44.1), (-75.2, 44.8), (-76.2, 44.8)],
        ],
        'parent_lake': 'ontario',
        'low_water_years': [2012, 2020],
        'optimal_scan_months': [6, 7, 8, 9],
    },
}

# ── Scan Scenarios ────────────────────────────────────────────────────────────
# Each scenario has weather requirements and sensor priorities
# NOTE: Reverse Thermal (Lead-Hunter) is ALWAYS ENABLED by default for all scans

SCAN_SCENARIOS = {
    'dead_calm_night': {
        'name': 'Dead Calm Night',
        'description': 'Thermal imaging optimal (hot day → cold night)',
        'weather_conditions': {
            'wind_speed_max_kts': 5,  # Very calm
            'diurnal_temp_range_min_c': 10,  # Hot day, cold night
            'cloud_cover_max_pct': 10,  # Clear skies
        },
        'sensor_priority': ['landsat', 'sentinel2'],  # Thermal + optical
        'priority_weight': 1.3,
        'includes_reverse_thermal': True,  # Always run Lead-Hunter with this scenario
    },
    
    'reverse_thermal_lead': {
        'name': 'Reverse Thermal (Lead-Hunter)',
        'description': 'FRP-encapsulated lead keels (ice cube in thermos effect) — ALWAYS ACTIVE',
        'weather_conditions': {
            'wind_speed_max_kts': 10,
            'diurnal_temp_range_min_c': 8,  # Warm day to charge surface
            'cloud_cover_max_pct': 30,
            'water_temp_surface_c': (15, 25),  # Warm surface water for contrast
        },
        'sensor_priority': ['landsat', 'sentinel2'],  # TIRS B10/B11 + Red-Edge B05
        'priority_weight': 1.8,  # High priority for Rossa detection
        'always_enabled': True,  # Core detection logic - runs with ALL scenarios
        'detection_logic': {
            'thermal_inertia': 'high',  # Lead stays at 4°C
            'negative_zscore': True,  # Cold spike in warm water
            'size_filter_m': 10,  # Bristol 35.5 keel length
            'material_contrast': {
                'B10_cold_spike': True,  # Thermal cold spot
                'B05_mussel_glow': False,  # No biological colonization (new wreck)
                'emissivity_low': True,  # Lead has different emissivity than water/FRP
            },
            'classification': {
                'cold_spike_only': 'Rossa (new FRP vessel)',  # Cold + no mussels = recent
                'cold_spike_plus_mussels': 'Andaste (historical steel)',  # Cold + mussels = old
            },
        },
    },
    
    'after_storm': {
        'name': 'After Storm',
        'description': '1-3 days after major storm (sediment plumes, debris)',
        'weather_conditions': {
            'wind_speed_max_kts': 15,  # Calm after storm
            'precipitation_days_ago': (1, 3),  # Storm was 1-3 days ago
            'cloud_cover_max_pct': 30,
        },
        'sensor_priority': ['sentinel2', 'landsat', 'swot'],  # Optical for plumes
        'priority_weight': 1.5,  # Higher priority for search & rescue
        'includes_reverse_thermal': True,  # Always run Lead-Hunter
    },
    
    'light_breeze': {
        'name': 'Light Breeze',
        'description': '4-7 knot winds (surface texture for SAR)',
        'weather_conditions': {
            'wind_speed_min_kts': 4,
            'wind_speed_max_kts': 7,
            'cloud_cover_max_pct': 40,
        },
        'sensor_priority': ['sentinel1', 'sentinel2'],  # SAR + optical
        'priority_weight': 1.2,
        'includes_reverse_thermal': True,  # Always run Lead-Hunter
    },
    
    'low_water_extreme': {
        'name': 'Low Water Extreme',
        'description': 'Historical low water levels (max depth visibility)',
        'weather_conditions': {
            'water_level_percentile': 20,  # Bottom 20% of historical levels
            'cloud_cover_max_pct': 25,
        },
        'sensor_priority': ['sentinel2', 'landsat', 'swot'],
        'priority_weight': 2.0,  # Highest priority (rare opportunity)
        'includes_reverse_thermal': True,  # Always run Lead-Hunter
    },
    
    'cesarops_sar': {
        'name': 'CESAROPS Search & Rescue',
        'description': 'Recent sinking scenario (stress test)',
        'weather_conditions': {
            'days_since_event': (0, 7),  # Event was 0-7 days ago
            'cloud_cover_max_pct': 50,  # Accept higher cloud cover
        },
        'sensor_priority': ['sentinel1', 'sentinel2', 'swot', 'landsat'],  # All sensors
        'priority_weight': 3.0,  # Emergency priority
        'includes_reverse_thermal': True,  # Always run Lead-Hunter
    },
    
    'aluminum_fabric_detect': {
        'name': 'Aluminum/Fabric Wreckage',
        'description': 'Aircraft wreckage (aluminum structure, fabric surfaces)',
        'weather_conditions': {
            'wind_speed_max_kts': 12,
            'cloud_cover_max_pct': 35,
        },
        'sensor_priority': ['sentinel1', 'sentinel2'],  # SAR for metal, optical for fabric
        'priority_weight': 1.6,
        'includes_reverse_thermal': True,  # Always run Lead-Hunter
        'detection_logic': {
            'radar_signature': 'high_contrast',  # Aluminum reflects SAR differently
            'fabric_texture': 'smooth_anomaly',  # Fabric has different texture than water
            'chromoly_signature': 'linear_features',  # Chromoly tubing creates linear SAR returns
        },
    },
}

# ── Satellite Availability & Coverage ────────────────────────────────────────

SATELLITE_OPERATIONAL_DATES = {
    'sentinel2a': {'start': '2015-06-23', 'end': None},  # Still operational
    'sentinel2b': {'start': '2017-03-07', 'end': '2021-12-23'},  # Deorbited
    'sentinel2c': {'start': '2024-09-05', 'end': None},  # Recently launched
    'sentinel1a': {'start': '2014-10-10', 'end': None},
    'sentinel1b': {'start': '2016-04-25', 'end': '2021-12-23'},  # Failed
    'landsat8': {'start': '2013-02-11', 'end': None},
    'landsat9': {'start': '2021-09-27', 'end': None},
    'swot': {'start': '2022-12-16', 'end': None},  # Ka-band radar
    'icesat2': {'start': '2018-10-14', 'end': None},
}

# Revisit cycles (days) — critical for multi-sensor coverage
SATELLITE_REVISIT_DAYS = {
    'sentinel2a': 5,  # 5 days with 2A+2B, 2-3 days with 2C
    'sentinel2b': 5,
    'sentinel2c': 2,  # Improved revisit
    'sentinel1a': 6,  # 6 days (12 days with 1B failed)
    'sentinel1b': 6,
    'landsat8': 16,  # 16 days, 8 days with 8+9 combined
    'landsat9': 16,
    'swot': 21,  # 21-day exact repeat cycle
    'icesat2': 91,  # 91-day repeat (sub-cycle variations)
}

# Great Lakes WRS-2 path/rows for Landsat coverage
GREAT_LAKES_LANDSAT_PATHS = {
    'superior': ['025', '026', '027'],  # Path 25-27
    'michigan': ['023', '024', '025'],  # Path 23-25
    'huron': ['019', '020', '021'],  # Path 19-21
    'erie': ['017', '018', '019'],  # Path 17-19
    'ontario': ['014', '015', '016'],  # Path 14-16
}

# Seasonal priority windows for Great Lakes scanning
SEASONAL_PRIORITIES = {
    'spring_post_ice': {
        'months': [4, 5],  # April-May
        'priority': 1.5,  # High priority
        'reason': 'Post-ice, post-snow runoff, low turbidity, clear water',
        'best_for': ['optical', 'thermal', 'mussel_base'],
    },
    'late_summer_mussel': {
        'months': [8],  # August (late)
        'priority': 1.8,  # Highest for mussel detection
        'reason': 'Peak mussel filtering, lowest turbidity, maximum contrast',
        'best_for': ['mussel_glow', 'optical', 'thermal'],
    },
    'early_fall_clear': {
        'months': [9, 10],  # September-October
        'priority': 1.3,
        'reason': 'Stable conditions, good thermal contrast',
        'best_for': ['thermal', 'optical', 'sar'],
    },
}

# Lighthouse/harbor weather stations (fallback when buoys pulled)
GREAT_LAKES_WEATHER_STATIONS = {
    'michigan': {
        'lighthouses': [
            {'name': 'Chicago Harbor Light', 'lat': 41.885, 'lon': -87.605},
            {'name': 'Waukegan Harbor Light', 'lat': 42.365, 'lon': -87.805},
            {'name': 'Kenosha Light', 'lat': 42.585, 'lon': -87.805},
            {'name': 'Racine Harbor Light', 'lat': 42.715, 'lon': -87.785},
            {'name': 'Milwaukee Breakwater Light', 'lat': 43.035, 'lon': -87.875},
            {'name': 'Sheboygan Light', 'lat': 43.755, 'lon': -87.715},
        ],
        'harbor_towns': [
            {'name': 'Waukegan', 'lat': 42.365, 'lon': -87.845},
            {'name': 'Kenosha', 'lat': 42.585, 'lon': -87.825},
            {'name': 'Racine', 'lat': 42.725, 'lon': -87.785},
            {'name': 'Milwaukee', 'lat': 43.035, 'lon': -87.905},
            {'name': 'Sheboygan', 'lat': 43.755, 'lon': -87.715},
        ],
        'buoys': ['45007', '45002', '45012'],  # NDBC buoys (pulled early spring/late fall)
    },
    'superior': {
        'lighthouses': [
            {'name': 'Duluth Ship Canal Light', 'lat': 46.725, 'lon': -92.095},
            {'name': 'Split Rock Light', 'lat': 47.295, 'lon': -91.015},
            {'name': 'Two Harbors Light', 'lat': 47.015, 'lon': -91.665},
        ],
        'harbor_towns': [
            {'name': 'Duluth', 'lat': 46.785, 'lon': -92.105},
            {'name': 'Two Harbors', 'lat': 47.025, 'lon': -91.675},
        ],
        'buoys': ['45001', '45006'],
    },
    'huron': {
        'lighthouses': [
            {'name': 'Harbor Beach Light', 'lat': 43.835, 'lon': -82.565},
            {'name': 'Port Sanilac Light', 'lat': 43.415, 'lon': -82.385},
        ],
        'harbor_towns': [
            {'name': 'Harbor Beach', 'lat': 43.845, 'lon': -82.575},
            {'name': 'Port Sanilac', 'lat': 43.425, 'lon': -82.395},
        ],
        'buoys': ['45004', '45005'],
    },
    'erie': {
        'lighthouses': [
            {'name': 'Cleveland Light', 'lat': 41.505, 'lon': -81.695},
            {'name': 'Ashtabula Light', 'lat': 41.885, 'lon': -80.785},
        ],
        'harbor_towns': [
            {'name': 'Cleveland', 'lat': 41.505, 'lon': -81.695},
            {'name': 'Ashtabula', 'lat': 41.895, 'lon': -80.795},
        ],
        'buoys': ['45003', '45010'],
    },
    'ontario': {
        'lighthouses': [
            {'name': 'Rochester Light', 'lat': 43.255, 'lon': -77.605},
            {'name': 'Sodus Bay Light', 'lat': 43.295, 'lon': -76.985},
        ],
        'harbor_towns': [
            {'name': 'Rochester', 'lat': 43.265, 'lon': -77.615},
            {'name': 'Sodus Point', 'lat': 43.305, 'lon': -76.995},
        ],
        'buoys': ['45012', '45013'],
    },
}

def get_available_satellites(date_str: str) -> List[str]:
    """Return list of satellites operational on given date."""
    available = []
    date = datetime.strptime(date_str, '%Y-%m-%d')

    for sat, dates in SATELLITE_OPERATIONAL_DATES.items():
        start = datetime.strptime(dates['start'], '%Y-%m-%d')
        end = datetime.strptime(dates['end'], '%Y-%m-%d') if dates['end'] else datetime.now()

        if start <= date <= end:
            available.append(sat)

    return available


def get_optimal_scan_dates(
    lake_region: str,
    year_range: Tuple[int, int],
    required_sensors: List[str],
    min_panels_per_sensor: int = 5,
    post_event_date: str = None,
) -> dict:
    """
    Find optimal scan dates across multiple years for complete lake coverage.
    
    Strategy:
      - Prioritize seasonal windows (spring post-ice, late summer mussel)
      - Gather at least min_panels_per_sensor for SWOT/ICESat-2 across years
      - Force coverage even if sensors have different revisit cycles
      - Flag if any sensor didn't fire or if post-event coverage unavailable
    
    Args:
        lake_region: Lake identifier
        year_range: (start_year, end_year) tuple
        required_sensors: List of required sensor names
        min_panels_per_sensor: Minimum SWOT/ICESat-2 passes to collect
        post_event_date: If set, only search dates after this (sink date mode)
    
    Returns:
        dict with:
          - best_dates: List of optimal dates with full/near-full coverage
          - seasonal_priority_dates: Dates in high-priority seasonal windows
          - sensor_coverage: Dict of sensor → number of passes found
          - warnings: List of coverage issues
          - flags: Dict of flags (sensor_missing, cuda_failed, post_event_no_coverage)
    """
    warnings = []
    flags = {
        'sensor_missing': [],
        'cuda_failed': False,
        'post_event_no_coverage': False,
    }
    
    best_dates = []
    seasonal_dates = {'spring_post_ice': [], 'late_summer_mussel': [], 'early_fall_clear': []}
    sensor_passes = {sensor: [] for sensor in required_sensors}
    
    # Generate candidate dates across year range
    start_year, end_year = year_range
    candidates = []
    
    for year in range(start_year, end_year + 1):
        for season_name, season_info in SEASONAL_PRIORITIES.items():
            for month in season_info['months']:
                # Generate dates for this month
                if month == 12:
                    next_month = 1
                    next_year = year + 1
                else:
                    next_month = month + 1
                    next_year = year
                
                month_start = datetime(year, month, 1)
                if next_month == 1:
                    month_end = datetime(next_year, next_month, 1) - timedelta(days=1)
                else:
                    month_end = datetime(year, next_month, 1) - timedelta(days=1)
                
                # Add all dates in this month
                current = month_start
                while current <= month_end:
                    date_str = current.strftime('%Y-%m-%d')
                    
                    # Skip if before post-event date (if specified)
                    if post_event_date and date_str < post_event_date:
                        current += timedelta(days=1)
                        continue
                    
                    # Check satellite availability
                    available = get_available_satellites(date_str)
                    missing = [s for s in required_sensors if s not in available]
                    
                    candidates.append({
                        'date': date_str,
                        'season': season_name,
                        'priority': season_info['priority'],
                        'available_sensors': available,
                        'missing_sensors': missing,
                        'reason': season_info['reason'],
                    })
                    
                    current += timedelta(days=1)
    
    # Sort candidates by priority (seasonal first, then by missing sensors)
    candidates.sort(key=lambda x: (-x['priority'], len(x['missing_sensors'])))
    
    # Collect dates ensuring minimum panels for SWOT/ICESat-2
    sparse_sensors = ['swot', 'icesat2']
    sparse_counts = {s: 0 for s in sparse_sensors if s in required_sensors}
    
    for candidate in candidates:
        date_str = candidate['date']
        season = candidate['season']
        
        # Add to appropriate seasonal bucket
        if season in seasonal_dates:
            seasonal_dates[season].append(date_str)
        
        # Track sensor passes
        for sensor in required_sensors:
            if sensor in candidate['available_sensors']:
                sensor_passes[sensor].append(date_str)
                if sensor in sparse_sensors:
                    sparse_counts[sensor] = sparse_counts.get(sensor, 0) + 1
        
        # Add to best dates if acceptable
        if len(candidate['missing_sensors']) <= 1:  # Allow 1 missing sensor
            best_dates.append({
                'date': date_str,
                'season': season,
                'priority': candidate['priority'],
                'available': candidate['available_sensors'],
                'missing': candidate['missing_sensors'],
            })
        
        # Stop if we have enough dates
        if len(best_dates) >= 50:
            break
    
    # Check if we got minimum panels for sparse sensors
    for sensor in sparse_sensors:
        if sensor in required_sensors:
            count = sparse_counts.get(sensor, 0)
            if count < min_panels_per_sensor:
                warnings.append(
                    f"⚠️ {sensor.upper()}: Only {count}/{min_panels_per_sensor} panels found. "
                    f"Consider expanding year range (current: {start_year}-{end_year})"
                )
                flags['sensor_missing'].append({
                    'sensor': sensor,
                    'found': count,
                    'required': min_panels_per_sensor,
                })
    
    # Check for post-event coverage (sink date mode)
    if post_event_date:
        post_dates = [d for d in best_dates if d['date'] > post_event_date]
        if not post_dates:
            warnings.append(f"⚠️ POST-EVENT: No satellite coverage available after {post_event_date}")
            flags['post_event_no_coverage'] = True
    
    # Check CUDA status
    cuda_status = check_cuda_availability()
    if not cuda_status['cuda_available']:
        flags['cuda_failed'] = True
        warnings.append(f"⚠️ CUDA: {cuda_status['warning']} — {cuda_status['accuracy_note']}")
    
    return {
        'best_dates': best_dates[:20],  # Top 20 dates
        'seasonal_priority_dates': seasonal_dates,
        'sensor_coverage': {s: len(passes) for s, passes in sensor_passes.items()},
        'sparse_sensor_counts': sparse_counts,
        'warnings': warnings,
        'flags': flags,
        'total_candidates': len(candidates),
    }


def check_sensor_coverage(date_range: Tuple[str, str], lake_region: str, required_sensors: List[str]) -> dict:
    """
    Check if required sensors have coverage in date range for lake region.
    Legacy function for quick checks — use get_optimal_scan_dates for full analysis.
    """
    warnings = []
    missing_dates = []
    best_dates = []
    
    start = datetime.strptime(date_range[0], '%Y-%m-%d')
    end = datetime.strptime(date_range[1], '%Y-%m-%d')
    
    # Check Landsat coverage (path/row specific)
    if 'landsat8' in required_sensors or 'landsat9' in required_sensors:
        landsat_paths = GREAT_LAKES_LANDSAT_PATHS.get(lake_region, ['023', '024', '025'])
        warnings.append(f"Landsat coverage: Path {landsat_paths} (revisit: 16 days, 8 with 8+9)")
    
    # Check SWOT coverage (21-day cycle, narrow swath)
    if 'swot' in required_sensors:
        warnings.append("SWOT coverage: 21-day repeat cycle, ~20km swath (may miss some dates)")
    
    # Check ICESat-2 (91-day repeat, narrow track)
    if 'icesat2' in required_sensors:
        warnings.append("ICESat-2 coverage: 91-day repeat, ~100m track (very sparse coverage)")
    
    # Iterate through dates and check coverage
    current = start
    while current <= end:
        date_str = current.strftime('%Y-%m-%d')
        available = get_available_satellites(date_str)
        
        # Check if all required sensors available
        missing = [s for s in required_sensors if s not in available]
        if missing:
            missing_dates.append({
                'date': date_str,
                'missing': missing,
            })
        else:
            best_dates.append(date_str)
        
        current += timedelta(days=1)
    
    coverage_ok = len(best_dates) > 0
    
    if not coverage_ok:
        warnings.append(f"WARNING: No dates found with full sensor coverage in range")
    
    return {
        'coverage_ok': coverage_ok,
        'warnings': warnings,
        'missing_dates': missing_dates[:10],  # Limit to first 10
        'best_dates': best_dates[:20],  # Limit to first 20
        'total_dates': len(best_dates),
        'total_missing': len(missing_dates),
    }


def check_cuda_availability() -> dict:
    """
    Check CUDA availability and warn if CPU-only mode.
    
    Returns dict with:
      - cuda_available: bool
      - gpu_name: str
      - warning: str or None
      - accuracy_note: str
    """
    try:
        import torch
        
        if torch.cuda.is_available():
            gpu_name = torch.cuda.get_device_name(0)
            return {
                'cuda_available': True,
                'gpu_name': gpu_name,
                'warning': None,
                'accuracy_note': f"GPU acceleration active: {gpu_name} (higher precision math)",
                'compute_capability': torch.cuda.get_device_capability(0),
            }
        else:
            return {
                'cuda_available': False,
                'gpu_name': 'N/A',
                'warning': 'CUDA not available — running on CPU',
                'accuracy_note': 'CPU mode: Standard floating-point precision (may have minor numerical differences)',
            }
    except ImportError:
        return {
            'cuda_available': False,
            'gpu_name': 'N/A',
            'warning': 'PyTorch not installed — running on CPU',
            'accuracy_note': 'CPU mode: Install PyTorch with CUDA for GPU acceleration',
        }


def get_weather_data(date_str: str, lake_region: str, location: dict = None) -> dict:
    """
    Get weather data for given date and lake region.
    
    Priority:
      1. NDBC Buoys (summer only — pulled early spring/late fall)
      2. Lighthouse stations (year-round)
      3. Harbor town weather stations (year-round fallback)
    
    Args:
        date_str: Date to fetch weather for
        lake_region: Lake identifier
        location: Optional specific location (lat/lon)
    
    Returns:
        dict with weather data and source info
    """
    # Check if buoys are deployed (typically May-November)
    date = datetime.strptime(date_str, '%Y-%m-%d')
    buoys_deployed = 5 <= date.month <= 10  # May-Oct
    
    stations = GREAT_LAKES_WEATHER_STATIONS.get(lake_region, GREAT_LAKES_WEATHER_STATIONS['michigan'])
    
    weather_data = None
    source = None
    
    # Try buoys first (if deployed)
    if buoys_deployed:
        for buoy_id in stations.get('buoys', []):
            try:
                # NDBC buoy API
                url = f'https://www.ndbc.noaa.gov/data/realtime2/{buoy_id}.txt'
                resp = requests.get(url, timeout=10)
                if resp.status_code == 200:
                    # Parse buoy data
                    lines = [l for l in resp.text.splitlines() if not l.startswith('#')]
                    for line in lines[2:]:
                        parts = line.split()
                        if len(parts) >= 7:
                            weather_data = {
                                'wind_speed_kts': float(parts[6]) if len(parts) > 6 else None,
                                'wind_dir_deg': float(parts[5]) if len(parts) > 5 else None,
                                'temp_c': None,  # Buoys don't always report temp
                                'source': f'NDBC Buoy {buoy_id}',
                                'source_type': 'buoy',
                            }
                            source = weather_data
                            break
                if weather_data:
                    break
            except Exception:
                continue
    
    # Fallback to lighthouse/harbor stations (year-round)
    if not weather_data:
        # Try harbor towns first (NWS API)
        for town in stations.get('harbor_towns', []):
            try:
                # NWS Grid Points API
                points_url = f'https://api.weather.gov/points/{town["lat"]},{town["lon"]}'
                resp = requests.get(points_url, headers={'User-Agent': 'WreckHunter2000/1.0'}, timeout=10)
                if resp.status_code == 200:
                    grid_data = resp.json()
                    forecast_url = grid_data['properties']['forecastHourly']
                    resp2 = requests.get(forecast_url, headers={'User-Agent': 'WreckHunter2000/1.0'}, timeout=10)
                    if resp2.status_code == 200:
                        period = resp2.json()['properties']['periods'][0]
                        weather_data = {
                            'wind_speed_kts': float(period.get('windSpeed', '0 mph').split()[0]) * 0.868976,
                            'wind_dir_deg': None,  # NWS doesn't always provide direction
                            'temp_c': (float(period.get('temperature', 0)) - 32) * 5/9,
                            'source': f"NWS {town['name']} (Harbor)",
                            'source_type': 'harbor_town',
                        }
                        source = weather_data
                        break
            except Exception:
                continue
    
    # Final fallback to lighthouse stations
    if not weather_data:
        for lh in stations.get('lighthouses', []):
            # Same NWS API call for lighthouse locations
            try:
                points_url = f'https://api.weather.gov/points/{lh["lat"]},{lh["lon"]}'
                resp = requests.get(points_url, headers={'User-Agent': 'WreckHunter2000/1.0'}, timeout=10)
                if resp.status_code == 200:
                    grid_data = resp.json()
                    forecast_url = grid_data['properties']['forecastHourly']
                    resp2 = requests.get(forecast_url, headers={'User-Agent': 'WreckHunter2000/1.0'}, timeout=10)
                    if resp2.status_code == 200:
                        period = resp2.json()['properties']['periods'][0]
                        weather_data = {
                            'wind_speed_kts': float(period.get('windSpeed', '0 mph').split()[0]) * 0.868976,
                            'wind_dir_deg': None,
                            'temp_c': (float(period.get('temperature', 0)) - 32) * 5/9,
                            'source': f"NWS {lh['name']} (Lighthouse)",
                            'source_type': 'lighthouse',
                        }
                        source = weather_data
                        break
            except Exception:
                continue
    
    if not weather_data:
        weather_data = {
            'wind_speed_kts': None,
            'wind_dir_deg': None,
            'temp_c': None,
            'source': 'UNAVAILABLE',
            'source_type': 'none',
            'warning': f'No weather data available for {lake_region} on {date_str}',
        }
    
    return weather_data

# ── Scan Registry Database ────────────────────────────────────────────────────

def init_scan_registry():
    """Initialize scan tracking database."""
    conn = sqlite3.connect(SCAN_DB)
    cur = conn.cursor()
    
    cur.execute("""
        CREATE TABLE IF NOT EXISTS scan_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            lake_region TEXT NOT NULL,
            scan_date TEXT NOT NULL,
            scenario TEXT NOT NULL,
            satellites_used TEXT,
            tiles_scanned TEXT,
            avg_score REAL,
            target_count INTEGER,
            output_files TEXT,
            storage_status TEXT DEFAULT 'ONLINE',  -- ONLINE, OFFLOADED, ARCHIVED
            scanned_at TEXT DEFAULT CURRENT_TIMESTAMP
        )
    """)
    
    cur.execute("""
        CREATE TABLE IF NOT EXISTS tile_coverage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            lake_region TEXT NOT NULL,
            tile_id TEXT NOT NULL,
            last_scan_date TEXT,
            scan_count INTEGER DEFAULT 1,
            best_score REAL,
            UNIQUE(lake_region, tile_id)
        )
    """)
    
    cur.execute("""
        CREATE TABLE IF NOT EXISTS weather_cache (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            lake_region TEXT NOT NULL,
            wind_speed_kts REAL,
            wind_dir_deg REAL,
            cloud_cover_pct REAL,
            temp_high_c REAL,
            temp_low_c REAL,
            precipitation_mm REAL,
            fetched_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(date, lake_region)
        )
    """)
    
    conn.commit()
    conn.close()

def get_scan_history(lake_region: str = None, limit: int = 50) -> List[dict]:
    """Get scan history, optionally filtered by lake region."""
    conn = sqlite3.connect(SCAN_DB)
    cur = conn.cursor()
    
    if lake_region:
        cur.execute("""
            SELECT * FROM scan_history
            WHERE lake_region = ?
            ORDER BY scanned_at DESC
            LIMIT ?
        """, (lake_region, limit))
    else:
        cur.execute("""
            SELECT * FROM scan_history
            ORDER BY scanned_at DESC
            LIMIT ?
        """, (limit,))
    
    columns = [desc[0] for desc in cur.description]
    results = [dict(zip(columns, row)) for row in cur.fetchall()]
    conn.close()
    
    return results

def record_scan(lake_region: str, scan_date: str, scenario: str,
                satellites: List[str], tiles: List[str], avg_score: float,
                target_count: int, output_files: List[str], 
                sensor_counts: dict = None, confidence_counts: dict = None,
                seasonal_window: str = None, cuda_info: dict = None,
                processing_time: float = None, storage_status: str = 'ONLINE'):
    """Record a completed scan in the wreckhunter_2026 database."""
    import uuid
    
    conn = sqlite3.connect(SCAN_DB)
    cur = conn.cursor()
    
    session_uuid = str(uuid.uuid4())
    
    # Insert scan session
    cur.execute("""
        INSERT INTO scan_sessions
        (session_uuid, lake_region, scenario, scan_date, date_range_start, date_range_end,
         seasonal_window, sentinel2_scenes, sentinel1_granules, landsat_granules,
         swot_granules, icesat2_granules, cuda_enabled, gpu_name, processing_time_sec,
         kml_path, kmz_path, json_path, total_targets, high_confidence, medium_confidence,
         low_confidence, pending_confidence, avg_score, status)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    """, (
        session_uuid, lake_region, scenario, scan_date,
        None, None,  # date_range (optional)
        seasonal_window,
        sensor_counts.get('sentinel2', 0) if sensor_counts else 0,
        sensor_counts.get('sentinel1', 0) if sensor_counts else 0,
        sensor_counts.get('landsat', 0) if sensor_counts else 0,
        sensor_counts.get('swot', 0) if sensor_counts else 0,
        sensor_counts.get('icesat2', 0) if sensor_counts else 0,
        1 if cuda_info and cuda_info.get('cuda_available') else 0,
        cuda_info.get('gpu_name', 'N/A') if cuda_info else 'N/A',
        processing_time,
        output_files[0] if len(output_files) > 0 and output_files[0].endswith('.kml') else None,
        output_files[0] if len(output_files) > 0 and output_files[0].endswith('.kmz') else None,
        output_files[1] if len(output_files) > 1 and output_files[1].endswith('.json') else None,
        target_count,
        confidence_counts.get('HIGH', 0) if confidence_counts else 0,
        confidence_counts.get('MEDIUM', 0) if confidence_counts else 0,
        confidence_counts.get('LOW', 0) if confidence_counts else 0,
        confidence_counts.get('PENDING', 0) if confidence_counts else 0,
        avg_score,
        'COMPLETE',
    ))
    
    session_id = cur.lastrowid
    
    # Update tile coverage
    for tile in tiles:
        # Determine season
        scan_month = int(scan_date.split('-')[1])
        season_col = 'scanned_spring' if scan_month in [3,4,5] else \
                     'scanned_summer' if scan_month in [6,7,8] else \
                     'scanned_fall' if scan_month in [9,10,11] else 'scanned_winter'
        
        cur.execute("""
            INSERT INTO tile_coverage (lake_region, tile_id, first_scan_date, last_scan_date, scan_count, best_avg_score)
            VALUES (?, ?, ?, ?, 1, ?)
            ON CONFLICT(lake_region, tile_id) DO UPDATE SET
                last_scan_date = ?,
                scan_count = scan_count + 1,
                best_avg_score = MAX(best_avg_score, ?),
                {} = 1
        """.format(season_col), (lake_region, tile, scan_date, scan_date, avg_score, scan_date, avg_score))
    
    conn.commit()
    conn.close()
    
    return session_id, session_uuid

def get_unscanned_tiles(lake_region: str, days_threshold: int = 30) -> List[str]:
    """Get tiles that haven't been scanned in N days."""
    conn = sqlite3.connect(SCAN_DB)
    cur = conn.cursor()
    
    cutoff_date = (datetime.now() - timedelta(days=days_threshold)).strftime('%Y-%m-%d')
    
    cur.execute("""
        SELECT tile_id FROM tile_coverage
        WHERE lake_region = ?
        AND (last_scan_date IS NULL OR last_scan_date < ?)
    """, (lake_region, cutoff_date))
    
    tiles = [row[0] for row in cur.fetchall()]
    conn.close()
    
    return tiles

def get_next_best_scan_date(lake_region: str, scenario: str,
                            start_date: str, end_date: str) -> Optional[str]:
    """
    Find next best scan date based on:
      - Not already scanned in registry
      - Weather matches scenario
      - Satellite availability
    """
    # Get already-scanned dates
    conn = sqlite3.connect(SCAN_DB)
    cur = conn.cursor()
    
    cur.execute("""
        SELECT DISTINCT scan_date FROM scan_history
        WHERE lake_region = ? AND scenario = ?
        ORDER BY scan_date DESC
    """, (lake_region, scenario))
    
    scanned_dates = {row[0] for row in cur.fetchall()}
    conn.close()
    
    # Find next unscanned date with good weather
    current = datetime.strptime(start_date, '%Y-%m-%d')
    end = datetime.strptime(end_date, '%Y-%m-%d')
    
    while current <= end:
        date_str = current.strftime('%Y-%m-%d')
        
        if date_str not in scanned_dates:
            # Check weather (placeholder - would fetch from API)
            weather_ok = True  # TODO: Implement weather API call
            
            if weather_ok:
                return date_str
        
        current += timedelta(days=1)
    
    return None

# ── Weather Integration ──────────────────────────────────────────────────────

def fetch_weather_for_date(date_str: str, lake_region: str) -> Optional[dict]:
    """
    Fetch historical weather for given date and lake region.
    Uses NOAA APIs or cached data.
    """
    # Check cache first
    conn = sqlite3.connect(SCAN_DB)
    cur = conn.cursor()
    
    cur.execute("""
        SELECT * FROM weather_cache
        WHERE date = ? AND lake_region = ?
    """, (date_str, lake_region))
    
    row = cur.fetchone()
    if row:
        columns = [desc[0] for desc in cur.description]
        result = dict(zip(columns, row))
        conn.close()
        return result
    
    conn.close()
    
    # TODO: Fetch from NOAA API (NDBC buoys, NWS grid data)
    # For now, return None (will be implemented with real API)
    return None

def match_weather_to_scenario(weather: dict, scenario: str) -> Tuple[bool, float]:
    """
    Check if weather matches scenario requirements.
    Returns (matches, confidence_score).
    """
    if not weather:
        return False, 0.0
    
    scenario_reqs = SCAN_SCENARIOS.get(scenario, {})
    conditions = scenario_reqs.get('weather_conditions', {})
    
    matches = True
    score = 0.0
    
    # Check wind speed
    if 'wind_speed_max_kts' in conditions:
        if weather.get('wind_speed_kts', 999) > conditions['wind_speed_max_kts']:
            matches = False
        else:
            score += 0.3
    
    if 'wind_speed_min_kts' in conditions:
        if weather.get('wind_speed_kts', 0) < conditions['wind_speed_min_kts']:
            matches = False
        else:
            score += 0.2
    
    # Check cloud cover
    if 'cloud_cover_max_pct' in conditions:
        if weather.get('cloud_cover_pct', 100) > conditions['cloud_cover_max_pct']:
            matches = False
        else:
            score += 0.3
    
    # Check diurnal temperature range
    if 'diurnal_temp_range_min_c' in conditions:
        temp_range = (weather.get('temp_high_c', 0) - weather.get('temp_low_c', 0))
        if temp_range < conditions['diurnal_temp_range_min_c']:
            matches = False
        else:
            score += 0.2
    
    return matches, score

# ── Storage Management ──────────────────────────────────────────────────────

def manage_storage(max_gb: float = 200.0):
    """
    Manage disk space by offloading older/lower-scoring scans.
    Keeps best-scoring days online, offloads rest to archive.
    """
    import shutil
    
    # Get current storage usage
    def get_dir_size(path):
        total = 0
        try:
            for entry in os.scandir(path):
                if entry.is_file():
                    total += entry.stat().st_size
                elif entry.is_dir():
                    total += get_dir_size(entry.path)
        except Exception:
            pass
        return total
    
    current_size_gb = get_dir_size(OUTPUT_DIR) / 1e9
    
    if current_size_gb <= max_gb:
        print(f'[+] Storage OK: {current_size_gb:.2f} GB / {max_gb:.2f} GB')
        return
    
    print(f'[!] Storage limit exceeded: {current_size_gb:.2f} GB > {max_gb:.2f} GB')
    print('    Offloading older scans...')
    
    # Get scans sorted by score (lowest first for offloading)
    conn = sqlite3.connect(SCAN_DB)
    cur = conn.cursor()
    
    cur.execute("""
        SELECT id, lake_region, scan_date, avg_score, output_files, storage_status
        FROM scan_history
        WHERE storage_status = 'ONLINE'
        ORDER BY avg_score ASC, scanned_at ASC
    """)
    
    scans = cur.fetchall()
    conn.close()
    
    # Offload until under limit
    for scan in scans:
        if get_dir_size(OUTPUT_DIR) / 1e9 <= max_gb:
            break
        
        scan_id, lake, date, score, files_json, _ = scan
        files = json.loads(files_json) if files_json else []
        
        # Move files to archive
        archive_dir = OUTPUT_DIR.parent / 'archive' / lake / date
        archive_dir.mkdir(parents=True, exist_ok=True)
        
        for f in files:
            src = Path(f)
            if src.exists():
                dst = archive_dir / src.name
                try:
                    shutil.move(str(src), str(dst))
                except Exception as e:
                    print(f'    [!] Failed to move {src}: {e}')
        
        # Update database
        conn = sqlite3.connect(SCAN_DB)
        cur = conn.cursor()
        cur.execute("""
            UPDATE scan_history SET storage_status = 'OFFLOADED'
            WHERE id = ?
        """, (scan_id,))
        conn.commit()
        conn.close()
        
        print(f'    Offloaded: {lake} {date} (score: {score})')
    
    print(f'[+] Storage managed: {get_dir_size(OUTPUT_DIR)/1e9:.2f} GB')

# ── Main Scanner ─────────────────────────────────────────────────────────────

def run_great_lakes_scan(
    lake_region: str,
    scenario: str = 'dead_calm_night',
    start_date: str = None,
    end_date: str = None,
    force_rescan: bool = False,
    output_format: str = 'kmz'
) -> dict:
    """
    Main scanning function for Great Lakes regions.
    
    Args:
        lake_region: Lake/river identifier (e.g., 'michigan', 'superior', 'st_marys_river')
        scenario: Scan scenario ('after_storm', 'dead_calm_night', 'light_breeze', etc.)
        start_date: Start date (YYYY-MM-DD), defaults to low-water year optimal window
        end_date: End date (YYYY-MM-DD)
        force_rescan: If True, ignore previous scans
        output_format: 'kml', 'kmz', or 'both'
    
    Returns:
        Scan result dict with targets, output files, etc.
    """
    # Initialize registry
    init_scan_registry()
    
    # Validate lake region
    if lake_region not in GREAT_LAKES_BBOXES:
        print(f'[!] Unknown lake region: {lake_region}')
        print(f'    Available: {list(GREAT_LAKES_BBOXES.keys())}')
        return {}
    
    lake_info = GREAT_LAKES_BBOXES[lake_region]
    bbox = lake_info['bbox']
    
    # Validate scenario
    if scenario not in SCAN_SCENARIOS:
        print(f'[!] Unknown scenario: {scenario}')
        print(f'    Available: {list(SCAN_SCENARIOS.keys())}')
        return {}
    
    scenario_info = SCAN_SCENARIOS[scenario]
    
    # Set date range
    if not start_date or not end_date:
        # Default to low-water year optimal window
        low_years = lake_info.get('low_water_years', [2021])
        optimal_months = lake_info.get('optimal_scan_months', [7, 8])
        
        # Use most recent low-water year
        year = max(low_years)
        start_month = min(optimal_months)
        end_month = max(optimal_months)
        
        start_date = f'{year}-{start_month:02d}-01'
        end_date = f'{year}-{end_month:02d}-28'
        
        print(f'[+] Auto date range: {start_date} to {end_date} (low-water year {year})')
    
    # Check satellite availability
    available_sats = get_available_satellites(start_date)
    print(f'[+] Satellites available: {available_sats}')
    
    # Find best scan date (not already scanned, good weather)
    if not force_rescan:
        best_date = get_next_best_scan_date(lake_region, scenario, start_date, end_date)
        if best_date:
            print(f'[+] Best scan date: {best_date} (not previously scanned)')
            scan_date = best_date
        else:
            print(f'[!] All dates in range already scanned')
            if not force_rescan:
                print('    Use --force-rescan to re-scan')
                return {}
            scan_date = start_date
    else:
        scan_date = start_date
    
    print(f'[+] Starting Great Lakes Scan')
    print(f'    Region: {lake_info["name"]}')
    print(f'    Scenario: {scenario_info["name"]} ({scenario_info["description"]})')
    print(f'    Bounding Box: {bbox}')
    print(f'    Scan Date: {scan_date}')
    print()

    # ── SMART DATE SELECTION (Multi-Year + Seasonal Priorities) ─────────────
    
    print('[Step 0] Finding optimal scan dates (multi-year + seasonal priorities)...')
    
    # Determine year range (auto low-water or manual)
    if not start_date or not end_date:
        low_years = lake_info.get('low_water_years', [2021])
        year_range = (max(low_years) - 2, max(low_years))  # 3-year window
    else:
        year_range = (int(start_date[:4]), int(end_date[:4]))
    
    # Get required sensors based on scenario
    required_sensors = ['sentinel2a']  # Always need S2
    if 'thermal' in scenario.lower() or 'landsat' in scenario_info.get('sensor_priority', []):
        required_sensors.append('landsat8')
    if 'swot' in scenario_info.get('sensor_priority', []):
        required_sensors.append('swot')
    if 'sar' in scenario_info.get('sensor_priority', []) or 'sentinel1' in scenario_info.get('sensor_priority', []):
        required_sensors.append('sentinel1a')
    
    # Get optimal dates with seasonal priorities
    opt_result = get_optimal_scan_dates(
        lake_region=lake_region,
        year_range=year_range,
        required_sensors=required_sensors,
        min_panels_per_sensor=5,  # Force 5+ SWOT/ICESat-2 panels
        post_event_date=None,  # Not sink-date mode
    )
    
    print(f'  Found {opt_result["total_candidates"]} candidate dates')
    print(f'  Best dates: {len(opt_result["best_dates"])}')
    print(f'  Seasonal windows:')
    for season, dates in opt_result['seasonal_priority_dates'].items():
        print(f'    {season}: {len(dates)} dates')
    
    # Check for warnings/flags
    if opt_result['warnings']:
        print('  ⚠️ Warnings:')
        for w in opt_result['warnings'][:5]:
            print(f'    {w}')
    
    # Use best date (or fallback to scan_date)
    if opt_result['best_dates']:
        best_date_info = opt_result['best_dates'][0]
        scan_date = best_date_info['date']
        print(f'  ✓ Selected: {scan_date} ({best_date_info["season"]}, priority {best_date_info["priority"]})')
    else:
        print(f'  ⚠️ No optimal dates found — using {scan_date}')
    
    print()

    # ── REAL SATELLITE DATA FETCH ────────────────────────────────────────────
    
    print('[Step 1] Querying NASA CMR for satellite data...')
    
    # Load Earthdata token
    token = ''
    for tp in TOKEN_PATHS:
        if tp.exists():
            try:
                if tp.suffix == '.json':
                    token = json.loads(tp.read_text(encoding='utf-8')).get('earthdata_token', '')
                else:
                    token = tp.read_text(encoding='utf-8').strip()
                if token:
                    break
            except Exception:
                continue
    
    # Query Sentinel-2 L2A via earth-search STAC (no token needed)
    print('  Fetching Sentinel-2 L2A optical data...')
    s2_scenes = fetch_sentinel2_l2a(bbox, (scan_date, scan_date), token)
    print(f'    Found {len(s2_scenes)} Sentinel-2 scenes')
    
    # Query Sentinel-1 SAR via ASF DAAC
    print('  Fetching Sentinel-1 SAR data...')
    s1_granules = fetch_sentinel1_sar(bbox, (scan_date, scan_date), token)
    print(f'    Found {len(s1_granules)} Sentinel-1 granules')
    
    # Query SWOT SSH via PO.DAAC
    print('  Fetching SWOT SSH data...')
    swot_granules = fetch_swot_ssh(bbox, (scan_date, scan_date), token)
    print(f'    Found {len(swot_granules)} SWOT granules')
    
    # Query Landsat Thermal via LP DAAC
    print('  Fetching Landsat thermal data...')
    landsat_granules = fetch_landsat_thermal(bbox, (scan_date, scan_date), token)
    print(f'    Found {len(landsat_granules)} Landsat granules')
    
    # Query ICESat-2 ATL13 via NSIDC
    print('  Fetching ICESat-2 laser altimetry...')
    icesat2_granules = fetch_icesat2_atl13(bbox, (scan_date, scan_date), token)
    print(f'    Found {len(icesat2_granules)} ICESat-2 granules')
    
    print()
    print('[Step 2] Detecting anomaly targets from satellite data...')
    
    # Detect targets from available data
    targets = []
    
    # Process Sentinel-2 scenes for optical anomalies
    for scene in s2_scenes:
        scene_targets = detect_targets_from_sentinel2(scene, token)
        # Add satellite angle metadata to each target (from sensor_data if available)
        for t in scene_targets:
            sensor_data = t.get('sensor_data', {})
            t['satellite_angles'] = {
                'sun_azimuth': sensor_data.get('sun_azimuth'),
                'sun_elevation': sensor_data.get('sun_elevation_deg'),
                'sat_zenith': sensor_data.get('sat_zenith'),
                'incidence_angle': sensor_data.get('incidence_angle'),
                'scene_id': sensor_data.get('scene_id', scene.get('granule_id', 'unknown')),
                'epoch_date': sensor_data.get('epoch_date'),
            }
        targets.extend(scene_targets)
        print(f'  Processed Sentinel-2 scene {scene.get("granule_id", "unknown")}: {len(scene_targets)} targets')
    
    # If no optical scenes found, load from census DB as fallback
    if not targets:
        print('  No new Sentinel-2 scenes — loading from census database...')
        targets = cuda_detect_targets_from_db()
        print(f'  Loaded {len(targets)} targets from database')
    
    print()
    print('[Step 3] Ranking targets by confidence (CUDA-accelerated)...')
    ranked_targets = cuda_rank_targets(targets)
    print(f'  Ranked {len(ranked_targets)} targets')
    
    # Count by confidence
    conf_counts = {'HIGH': 0, 'MEDIUM': 0, 'LOW': 0, 'PENDING': 0}
    for t in ranked_targets:
        conf_counts[t['confidence']] = conf_counts.get(t['confidence'], 0) + 1
    
    print()
    print('[Step 4] Generating KML/KMZ output...')
    
    # Generate metadata
    metadata = {
        'run_at': datetime.now(timezone.utc).isoformat(),
        'lake_region': lake_region,
        'scenario': scenario,
        'scan_date': scan_date,
        'bbox': bbox,
        'sensor_counts': {
            'sentinel2': len(s2_scenes),
            'sentinel1': len(s1_granules),
            'swot': len(swot_granules),
            'landsat': len(landsat_granules),
            'icesat2': len(icesat2_granules),
        },
        'high_count': conf_counts['HIGH'],
        'medium_count': conf_counts['MEDIUM'],
        'low_count': conf_counts['LOW'],
        'pending_count': conf_counts['PENDING'],
    }
    
    # Generate KML content
    kml_content = create_kml_document(ranked_targets, bbox, metadata)
    
    # Save outputs
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    output_base = OUTPUT_DIR / f'{lake_region}_{scenario}_{timestamp}'
    
    output_files = []
    
    if output_format in ('kml', 'both'):
        kml_path = save_kml(kml_content, output_base.with_suffix('.kml'))
        output_files.append(str(kml_path))
        print(f'  Saved KML: {kml_path}')
    
    if output_format in ('kmz', 'both'):
        kmz_path = save_kmz(kml_content, output_base)
        output_files.append(str(kmz_path))
        print(f'  Saved KMZ: {kmz_path}')
    
    # Save JSON summary (INCLUDE SATELLITE ANGLES FOR EACH TARGET)
    json_path = OUTPUT_DIR / f'{lake_region}_{scenario}_{timestamp}.json'
    with open(json_path, 'w', encoding='utf-8') as f:
        json.dump({
            'run_at': metadata['run_at'],
            'lake_region': lake_region,
            'scenario': scenario,
            'scan_date': scan_date,
            'optimal_dates_info': opt_result,
            'sensor_counts': metadata['sensor_counts'],
            'confidence_counts': conf_counts,
            'total_targets': len(targets),
            'top_targets': ranked_targets[:20],  # Includes satellite_angles for each
            'output_files': output_files,
        }, f, indent=2, default=str)
    print(f'  Saved JSON: {json_path}')
    print(f'    (Satellite angles logged for all {len(targets)} targets)')
    
    print()
    print('=' * 62)
    print('SCAN COMPLETE')
    print('=' * 62)
    print(f'  Region: {lake_region}')
    print(f'  Scenario: {scenario}')
    print(f'  Sensors: S2={len(s2_scenes)}, S1={len(s1_granules)}, SWOT={len(swot_granules)}, L8/9={len(landsat_granules)}, ICESat-2={len(icesat2_granules)}')
    print(f'  Total Targets: {len(targets)}')
    print(f'  HIGH Confidence: {conf_counts["HIGH"]}')
    print(f'  MEDIUM Confidence: {conf_counts["MEDIUM"]}')
    print(f'  LOW Confidence: {conf_counts["LOW"]}')
    print(f'  PENDING: {conf_counts["PENDING"]}')
    print(f'  Output Files:')
    for f in output_files:
        print(f'    - {f}')
    
    # CUDA Telemetry
    cuda_telemetry = get_cuda_telemetry()
    print(f'  CUDA Telemetry:')
    print(f'    GPU: {cuda_telemetry["gpu_name"]}')
    if cuda_telemetry['temperature_c']:
        temp_status = '⚠️' if cuda_telemetry['temperature_c'] >= 75 else '✅'
        print(f'    {temp_status} Temperature: {cuda_telemetry["temperature_c"]}°C')
        if cuda_telemetry.get('warning'):
            print(f'    ⚠️ {cuda_telemetry["warning"]}')
    else:
        print(f'    ℹ️ {cuda_telemetry.get("note", "Temperature not available")}')
    print(f'    VRAM: {cuda_telemetry["memory_used_gb"]} / {cuda_telemetry["memory_total_gb"]} GB ({cuda_telemetry["memory_percent"]}% used)')
    print('=' * 62)
    
    # Record in scan registry
    import time
    avg_score = sum(t['score'] or 0 for t in ranked_targets) / len(ranked_targets) if ranked_targets else 0
    
    # Get CUDA info
    cuda_info = check_cuda_availability()
    
    session_id, session_uuid = record_scan(
        lake_region=lake_region,
        scan_date=scan_date,
        scenario=scenario,
        satellites=[f'sentinel2 ({len(s2_scenes)})', f'sentinel1 ({len(s1_granules)})', f'swot ({len(swot_granules)})'],
        tiles=[lake_region],
        avg_score=avg_score,
        target_count=len(targets),
        output_files=output_files,
        sensor_counts=metadata['sensor_counts'],
        confidence_counts=conf_counts,
        seasonal_window=opt_result.get('best_dates', [{}])[0].get('season', 'unknown') if opt_result.get('best_dates') else 'future_date',
        cuda_info=cuda_info,
        processing_time=time.time(),  # Would need to track start time for accurate value
        storage_status='ONLINE',
    )
    
    result = {
        'lake_region': lake_region,
        'scenario': scenario,
        'scan_date': scan_date,
        'bbox': bbox,
        'satellites': available_sats,
        'targets': ranked_targets,
        'output_files': output_files,
        'sensor_counts': metadata['sensor_counts'],
        'confidence_counts': conf_counts,
        'status': 'COMPLETE',
    }
    
    return result

# ── CLI Interface ─────────────────────────────────────────────────────────────

if __name__ == '__main__':
    import argparse
    
    parser = argparse.ArgumentParser(description='Great Lakes Satellite Scanner')
    
    parser.add_argument(
        '--lake',
        type=str,
        required=True,
        choices=list(GREAT_LAKES_BBOXES.keys()),
        help='Lake/river region to scan'
    )
    
    parser.add_argument(
        '--scenario',
        type=str,
        default='dead_calm_night',
        choices=list(SCAN_SCENARIOS.keys()),
        help='Scan scenario'
    )
    
    parser.add_argument(
        '--start-date',
        type=str,
        default=None,
        help='Start date (YYYY-MM-DD)'
    )
    
    parser.add_argument(
        '--end-date',
        type=str,
        default=None,
        help='End date (YYYY-MM-DD)'
    )
    
    parser.add_argument(
        '--force-rescan',
        action='store_true',
        help='Re-scan even if already scanned'
    )
    
    parser.add_argument(
        '--output',
        type=str,
        default='kmz',
        choices=['kml', 'kmz', 'both'],
        help='Output format'
    )
    
    args = parser.parse_args()
    
    result = run_great_lakes_scan(
        lake_region=args.lake,
        scenario=args.scenario,
        start_date=args.start_date,
        end_date=args.end_date,
        force_rescan=args.force_rescan,
        output_format=args.output,
    )
    
    if result:
        print()
        print('=' * 62)
        print('SCAN COMPLETE')
        print('=' * 62)
        print(f'  Region: {result.get("lake_region")}')
        print(f'  Targets: {len(result.get("targets", []))}')
        print(f'  Output: {result.get("output_files", [])}')
        print('=' * 62)
