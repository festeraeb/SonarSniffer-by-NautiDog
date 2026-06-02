#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
CESAROPS Detection Sorter - Filter Layer with Release Control

Sits between database and output (KMZ, Web, MBTiles).
Toggle filters on/off via web UI or API to control what gets displayed.

Usage:
    # Basic usage
    sorter = DetectionSorter(db_path)
    sites = sorter.apply()
    
    # With filters
    sorter.set_filter('min_confidence', 0.8)
    sorter.set_filter('release_status', ['CONFIRMED', 'PUBLIC'])
    sites = sorter.apply()
    
    # Admin mode (see everything)
    sorter = DetectionSorter(db_path, admin_mode=True)
    sites = sorter.apply()
"""

import sqlite3
import json
import math
from pathlib import Path
from datetime import datetime
from typing import Optional, List, Dict, Any, Tuple
from statistics import pstdev

# ============================================================================
# CONFIGURATION
# ============================================================================

# Bounding box presets
BBOX_PRESETS = {
    'MICHIGAN_SOUTH': (42.4, -87.2, 43.0, -86.5),
    'MICHIGAN_NORTH': (44.0, -86.5, 45.5, -85.5),
    'ERIE': (41.5, -82.5, 42.5, -80.5),
    'HURON': (43.5, -82.5, 45.5, -81.5),
    'SUPERIOR': (46.5, -91.0, 48.0, -84.0),
}

# API Keys (in production, move to environment variables)
API_KEYS = {
    'cesarops_admin_key_2026': ['view', 'filter', 'release', 'export', 'admin'],
    'cesarops_collab_key_2026': ['view', 'filter', 'export'],
    'cesarops_family_key_2026': ['view'],
}

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

def is_nan(value) -> bool:
    """Check if value is NaN or None"""
    return value is None or (isinstance(value, float) and math.isnan(value))

def is_ud(value) -> bool:
    """Check if value is UD (Undetectable)"""
    return value == 'UD'

def haversine_distance(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    """Calculate distance between two points in meters"""
    R = 6371000  # Earth radius in meters
    
    lat1_rad = math.radians(lat1)
    lat2_rad = math.radians(lat2)
    delta_lat = math.radians(lat2 - lat1)
    delta_lon = math.radians(lon2 - lon1)
    
    a = math.sin(delta_lat/2)**2 + math.cos(lat1_rad) * math.cos(lat2_rad) * math.sin(delta_lon/2)**2
    c = 2 * math.atan2(math.sqrt(a), math.sqrt(1-a))
    
    return R * c

def calculate_spatial_stddev(events: List[Dict]) -> Tuple[float, float, float]:
    """
    Calculate spatial standard deviation of detection events
    
    Returns: (stddev_meters, centroid_lat, centroid_lon)
    """
    if len(events) < 2:
        return 0.0, events[0]['lat'], events[0]['lon']
    
    lats = [e['lat'] for e in events]
    lons = [e['lon'] for e in events]
    
    # Calculate centroid
    centroid_lat = sum(lats) / len(lats)
    centroid_lon = sum(lons) / len(lons)
    
    # Calculate distances from centroid
    distances = [
        haversine_distance(centroid_lat, centroid_lon, lat, lon)
        for lat, lon in zip(lats, lons)
    ]
    
    # Standard deviation
    stddev = pstdev(distances)
    
    return stddev, centroid_lat, centroid_lon

def calculate_confidence(site_data: Dict) -> Tuple[float, str]:
    """
    Calculate confidence score based on:
    1. Detection count (more = higher)
    2. Spatial consistency (tight = higher)
    3. Tool agreement (multiple tools = higher)
    4. Temporal persistence (found over time = higher)
    
    Returns: (confidence_score, confidence_level)
    """
    total_detections = site_data.get('total_detections', 1)
    spatial_stddev = site_data.get('spatial_stddev_m', 0)
    tools_used = site_data.get('tools_used', [])
    first_detected = site_data.get('first_detected')
    last_detected = site_data.get('last_detected')
    
    # 1. Count score (logarithmic)
    count_score = min(0.9, 0.2 * math.log2(total_detections + 1))
    
    # 2. Spatial consistency
    if spatial_stddev < 5:
        spatial_score = 1.0
    elif spatial_stddev < 10:
        spatial_score = 0.9
    elif spatial_stddev < 25:
        spatial_score = 0.7
    elif spatial_stddev < 50:
        spatial_score = 0.5
    else:
        spatial_score = 0.3
    
    # 3. Tool agreement
    tool_count = len(tools_used)
    if tool_count >= 3:
        tool_score = 1.0
    elif tool_count == 2:
        tool_score = 0.8
    elif tool_count == 1:
        tool_score = 0.6
    else:
        tool_score = 0.3
    
    # 4. Temporal persistence
    if first_detected and last_detected and first_detected != last_detected:
        temporal_score = 0.9
    else:
        temporal_score = 0.5
    
    # Weighted average
    confidence = (
        count_score * 0.35 +
        spatial_score * 0.25 +
        tool_score * 0.25 +
        temporal_score * 0.15
    )
    
    confidence = min(1.0, confidence)
    
    # Level label
    if confidence >= 0.9:
        level = "VERY_HIGH"
    elif confidence >= 0.7:
        level = "HIGH"
    elif confidence >= 0.5:
        level = "MEDIUM"
    else:
        level = "LOW"
    
    return confidence, level

# ============================================================================
# DATA CLASSES
# ============================================================================

class DetectionSite:
    """Represents a unique detection site with multiple events"""
    
    def __init__(self, row: sqlite3.Row):
        self.site_id = row['site_id']
        self.lat = row['lat']
        self.lon = row['lon']
        self.grid_ref = row.get('grid_ref', f"WH2K-{int(abs(self.lat)*1000):04d}-{int(abs(self.lon)*1000):04d}")
        
        # Core metrics
        self.first_detected = row.get('first_detected')
        self.last_detected = row.get('last_detected')
        self.total_detections = row.get('total_detections', 1)
        
        # Confidence (calculated or from DB)
        self.confidence_score = row.get('confidence_score', 0.0)
        self.confidence_level = row.get('confidence_level', 'LOW')
        
        # Spatial
        self.spatial_stddev_m = row.get('spatial_stddev_m', 0.0)
        self.is_debris_field = bool(row.get('is_debris_field', 0))
        self.debris_field_radius_m = row.get('debris_field_radius_m', 0.0)
        
        # Tools
        tools_json = row.get('tools_used', '[]')
        self.tools_used = json.loads(tools_json) if tools_json else []
        self.tool_count = len(self.tools_used)
        
        # Release status
        self.release_status = row.get('release_status', 'INTERNAL')
        self.released_at = row.get('released_at')
        self.released_by = row.get('released_by')
        
        # Classification
        self.classification = row.get('classification', 'UNKNOWN')
        self.identified = bool(row.get('identified', 0))
        self.wreck_name = row.get('wreck_name')
        self.wreck_year = row.get('wreck_year')
        self.wreck_type = row.get('wreck_type')
        self.material = row.get('material', 'unknown')
        self.bbox_id = row.get('bbox_id')
        
        # Sensor data (may be NaN/UD)
        self.thermal_zscore = row.get('thermal_zscore')
        self.sar_coherence = row.get('sar_coherence')
        self.swot_height_m = row.get('swot_height_m')
        
        # Processing metadata
        self.processor_id = row.get('processor_id')
        self.algorithm_version = row.get('algorithm_version')
    
    def to_dict(self) -> Dict[str, Any]:
        return {
            'site_id': self.site_id,
            'lat': self.lat,
            'lon': self.lon,
            'grid_ref': self.grid_ref,
            'classification': self.classification,
            'identified': self.identified,
            'wreck_name': self.wreck_name,
            'total_detections': self.total_detections,
            'confidence_score': self.confidence_score,
            'confidence_level': self.confidence_level,
            'spatial_stddev_m': self.spatial_stddev_m,
            'is_debris_field': self.is_debris_field,
            'tools_used': self.tools_used,
            'release_status': self.release_status,
            'material': self.material,
            'bbox_id': self.bbox_id,
            'first_detected': self.first_detected,
            'last_detected': self.last_detected,
        }

# ============================================================================
# SORTER CLASS
# ============================================================================

class DetectionSorter:
    """
    Filter and sort detections before feeding to output (KMZ, Web, MBTiles)
    """
    
    def __init__(self, db_path: Path, admin_mode: bool = False, api_key: str = None):
        if not db_path.exists():
            raise FileNotFoundError(f"Database not found: {db_path}")
        
        self.db_path = db_path
        self.conn = sqlite3.connect(str(db_path))
        self.conn.row_factory = sqlite3.Row
        self.admin_mode = admin_mode
        self.api_key = api_key
        
        # Default filters
        self.filters = {
            'min_confidence': 0.0,
            'max_confidence': 1.0,
            'tools': [],
            'identified': None,
            'material': [],
            'bbox': None,
            'bbox_custom': None,
            'date_range': None,
            'min_consistency': 0.0,
            'min_detections': 1,
            'classification': [],
            'release_status': ['CONFIRMED', 'PUBLIC'] if not admin_mode else [],
            'is_debris_field': None,
            'max_stddev_m': None,
            'min_debris_radius_m': None,
            'processor_id': None,
            'algorithm_version': None,
            'has_thermal': None,
            'has_sar': None,
            'has_swot': None,
        }
    
    def set_filter(self, key: str, value):
        """Set a single filter"""
        if key in self.filters:
            self.filters[key] = value
        return self
    
    def clear_filters(self):
        """Reset all filters to default (show everything for admin, released for public)"""
        self.filters = {
            'min_confidence': 0.0,
            'max_confidence': 1.0,
            'tools': [],
            'identified': None,
            'material': [],
            'bbox': None,
            'bbox_custom': None,
            'date_range': None,
            'min_consistency': 0.0,
            'min_detections': 1,
            'classification': [],
            'release_status': ['CONFIRMED', 'PUBLIC'] if not self.admin_mode else [],
            'is_debris_field': None,
            'max_stddev_m': None,
            'min_debris_radius_m': None,
            'processor_id': None,
            'algorithm_version': None,
            'has_thermal': None,
            'has_sar': None,
            'has_swot': None,
        }
        return self
    
    def enable_admin_mode(self, api_key: str) -> bool:
        """Enable admin mode with valid API key"""
        if api_key in API_KEYS:
            self.admin_mode = True
            self.api_key = api_key
            self.filters['release_status'] = []  # Show all statuses
            return True
        return False
    
    def get_permissions(self) -> List[str]:
        """Get permissions for current API key"""
        if self.api_key and self.api_key in API_KEYS:
            return API_KEYS[self.api_key]
        return []
    
    def apply(self) -> List[DetectionSite]:
        """Query database with active filters"""
        
        query = """
            SELECT * FROM detection_sites
            WHERE 1=1
        """
        params = []
        
        # Confidence filters
        if self.filters['min_confidence'] > 0:
            query += " AND confidence_score >= ?"
            params.append(self.filters['min_confidence'])
        
        if self.filters['max_confidence'] < 1.0:
            query += " AND confidence_score <= ?"
            params.append(self.filters['max_confidence'])
        
        # Tools filter
        if self.filters['tools']:
            query += " AND ("
            tool_conditions = []
            for tool in self.filters['tools']:
                tool_conditions.append("tools_used LIKE ?")
                params.append(f'%{tool}%')
            query += " OR ".join(tool_conditions)
            query += ")"
        
        # Identified filter
        if self.filters['identified'] is not None:
            query += " AND identified = ?"
            params.append(1 if self.filters['identified'] else 0)
        
        # Material filter
        if self.filters['material']:
            query += " AND material IN (" + ','.join(['?'] * len(self.filters['material'])) + ")"
            params.extend(self.filters['material'])
        
        # BBOX preset filter
        if self.filters['bbox']:
            query += " AND bbox_id = ?"
            params.append(self.filters['bbox'])
        
        # Custom BBOX filter
        if self.filters['bbox_custom']:
            min_lat, min_lon, max_lat, max_lon = self.filters['bbox_custom']
            query += " AND lat >= ? AND lat <= ? AND lon >= ? AND lon <= ?"
            params.extend([min_lat, max_lat, min_lon, max_lon])
        
        # Release status filter
        if self.filters['release_status']:
            query += " AND release_status IN (" + ','.join(['?'] * len(self.filters['release_status'])) + ")"
            params.extend(self.filters['release_status'])
        
        # Consistency filter
        if self.filters['min_consistency'] > 0:
            query += " AND consistency_score >= ?"
            params.append(self.filters['min_consistency'])
        
        # Min detections filter
        if self.filters['min_detections'] > 1:
            query += " AND total_detections >= ?"
            params.append(self.filters['min_detections'])
        
        # Classification filter
        if self.filters['classification']:
            query += " AND classification IN (" + ','.join(['?'] * len(self.filters['classification'])) + ")"
            params.extend(self.filters['classification'])
        
        # Debris field filter
        if self.filters['is_debris_field'] is not None:
            query += " AND is_debris_field = ?"
            params.append(1 if self.filters['is_debris_field'] else 0)
        
        # Max stddev filter
        if self.filters['max_stddev_m'] is not None:
            query += " AND spatial_stddev_m <= ?"
            params.append(self.filters['max_stddev_m'])
        
        # Min debris radius filter
        if self.filters['min_debris_radius_m'] is not None:
            query += " AND debris_field_radius_m >= ?"
            params.append(self.filters['min_debris_radius_m'])
        
        # Processor filter
        if self.filters['processor_id']:
            query += " AND processor_id = ?"
            params.append(self.filters['processor_id'])
        
        # Algorithm filter
        if self.filters['algorithm_version']:
            query += " AND algorithm_version = ?"
            params.append(self.filters['algorithm_version'])
        
        # Thermal data filter
        if self.filters['has_thermal'] is True:
            query += " AND thermal_zscore IS NOT NULL AND thermal_zscore != 'UD' AND NOT (CAST(thermal_zscore AS TEXT) GLOB '[Nn][Aa][Nn]')"
        elif self.filters['has_thermal'] is False:
            query += " AND (thermal_zscore IS NULL OR thermal_zscore = 'UD' OR CAST(thermal_zscore AS TEXT) GLOB '[Nn][Aa][Nn]')"
        
        # SAR data filter
        if self.filters['has_sar'] is True:
            query += " AND sar_coherence IS NOT NULL AND sar_coherence != 'UD' AND NOT (CAST(sar_coherence AS TEXT) GLOB '[Nn][Aa][Nn]')"
        elif self.filters['has_sar'] is False:
            query += " AND (sar_coherence IS NULL OR sar_coherence = 'UD' OR CAST(sar_coherence AS TEXT) GLOB '[Nn][Aa][Nn]')"
        
        # Order by confidence (highest first)
        query += " ORDER BY confidence_score DESC, total_detections DESC"
        
        # Execute query
        cursor = self.conn.cursor()
        cursor.execute(query, params)
        rows = cursor.fetchall()
        
        # Convert to DetectionSite objects
        sites = [DetectionSite(row) for row in rows]
        
        return sites
    
    def get_stats(self) -> Dict[str, Any]:
        """Get statistics about current filtered results"""
        sites = self.apply()
        
        if not sites:
            return {
                'total_sites': 0,
                'identified': 0,
                'unidentified': 0,
                'tools': {},
                'materials': {},
                'classifications': {},
                'release_status': {},
                'avg_confidence': 0.0,
            }
        
        # Count by categories
        identified = sum(1 for s in sites if s.identified)
        tools = {}
        materials = {}
        classifications = {}
        release_status = {}
        
        for site in sites:
            # Tools
            for tool in site.tools_used:
                tools[tool] = tools.get(tool, 0) + 1
            
            # Materials
            mat = site.material or 'unknown'
            materials[mat] = materials.get(mat, 0) + 1
            
            # Classifications
            cls = site.classification or 'UNKNOWN'
            classifications[cls] = classifications.get(cls, 0) + 1
            
            # Release status
            status = site.release_status or 'INTERNAL'
            release_status[status] = release_status.get(status, 0) + 1
        
        return {
            'total_sites': len(sites),
            'identified': identified,
            'unidentified': len(sites) - identified,
            'tools': tools,
            'materials': materials,
            'classifications': classifications,
            'release_status': release_status,
            'avg_confidence': sum(s.confidence_score for s in sites) / len(sites),
            'avg_consistency': sum(s.confidence_score for s in sites) / len(sites),
            'debris_fields': sum(1 for s in sites if s.is_debris_field),
            'point_targets': sum(1 for s in sites if not s.is_debris_field),
        }
    
    def close(self):
        """Close database connection"""
        self.conn.close()
    
    def __enter__(self):
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()


# ============================================================================
# CONVENIENCE FUNCTIONS
# ============================================================================

def quick_filter(db_path: Path, **filters) -> List[DetectionSite]:
    """
    Quick one-line filtering
    
    Usage:
        sites = quick_filter(db_path, min_confidence=0.8, tools=['M2200'])
    """
    with DetectionSorter(db_path) as sorter:
        for key, value in filters.items():
            sorter.set_filter(key, value)
        return sorter.apply()


# ============================================================================
# MAIN (TEST)
# ============================================================================

if __name__ == "__main__":
    import sys
    
    db_path = Path(__file__).parent / "wreckhunter2000" / "LAKE_MICHIGAN_CENSUS_2026.db"
    
    if not db_path.exists():
        print(f"Database not found: {db_path}")
        print("Run cesarops_engine.py first to create database")
        sys.exit(1)
    
    print("="*70)
    print("CESAROPS DETECTION SORTER - TEST")
    print("="*70)
    
    with DetectionSorter(db_path, admin_mode=True) as sorter:
        # Test 1: All sites (admin mode)
        print("\n[1] All detections (admin mode, no filters):")
        stats = sorter.get_stats()
        print(f"  Total sites: {stats['total_sites']}")
        print(f"  Identified: {stats['identified']}")
        print(f"  Unidentified: {stats['unidentified']}")
        print(f"  Release status: {stats['release_status']}")
        
        # Test 2: High confidence only
        print("\n[2] High confidence (>0.7):")
        sorter.set_filter('min_confidence', 0.7)
        stats = sorter.get_stats()
        print(f"  Total sites: {stats['total_sites']}")
        print(f"  Avg confidence: {stats['avg_confidence']*100:.1f}%")
        
        # Test 3: Clear filters
        print("\n[3] Clear filters:")
        sorter.clear_filters()
        stats = sorter.get_stats()
        print(f"  Total sites: {stats['total_sites']}")
    
    print("\n" + "="*70)
    print("✓ Sorter test complete")
    print("="*70)
