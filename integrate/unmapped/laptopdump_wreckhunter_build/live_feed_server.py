#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
CESAROPS Live Feed Server

Flask server providing:
- Web UI for sorter control (cesarops.com/sorter)
- API endpoints for filtering
- Live KMZ feed for Google Earth
- Release status management

Usage:
    python live_feed_server.py
    
Access:
- Web UI: http://localhost:8080/sorter
- Live KMZ: http://localhost:8080/feed.kmz
- API: http://localhost:8080/api/sorter
"""

import os
import sys
import sqlite3
import json
from pathlib import Path
from datetime import datetime
from flask import Flask, request, jsonify, render_template, session, Response, make_response

# Import our sorter
from detection_sorter import DetectionSorter, DetectionSite, API_KEYS, calculate_confidence

# ============================================================================
# CONFIGURATION
# ============================================================================

app = Flask(__name__)
app.secret_key = os.environ.get('FLASK_SECRET_KEY', 'cesarops_dev_secret_key_2026')

# Database path
DB_PATH = Path(__file__).parent / "wreckhunter2000" / "LAKE_MICHIGAN_CENSUS_2026.db"

# ============================================================================
# KMZ GENERATOR (Simple in-memory, no file I/O)
# ============================================================================

def generate_kmz(sites: list, title: str = "CESAROPS Live Feed") -> bytes:
    """
    Generate KMZ data in-memory (no file I/O)
    
    Returns KMZ as bytes for direct HTTP response
    """
    import simplekml
    
    kml = simplekml.Kml()
    kml.document.name = title
    
    # Group by confidence level
    folders = {
        'VERY_HIGH': kml.newfolder(name=f"🟢 Very High Confidence ({len([s for s in sites if s.confidence_level == 'VERY_HIGH'])})"),
        'HIGH': kml.newfolder(name=f"🟡 High Confidence ({len([s for s in sites if s.confidence_level == 'HIGH'])})"),
        'MEDIUM': kml.newfolder(name=f"🟠 Medium Confidence ({len([s for s in sites if s.confidence_level == 'MEDIUM'])})"),
        'LOW': kml.newfolder(name=f"🔴 Low Confidence ({len([s for s in sites if s.confidence_level == 'LOW'])})"),
    }
    
    for site in sites:
        folder = folders.get(site.confidence_level, folders['LOW'])
        
        # Create placemark
        placemark = folder.newpoint(
            name=f"{site.grid_ref} - {site.classification}",
            coords=[(site.lon, site.lat)]
        )
        
        # Icon color by confidence
        icon_colors = {
            'VERY_HIGH': 'http://maps.google.com/mapfiles/kml/paddle/grn-circle.png',
            'HIGH': 'http://maps.google.com/mapfiles/kml/paddle/ylw-circle.png',
            'MEDIUM': 'http://maps.google.com/mapfiles/kml/paddle/ltblu-circle.png',
            'LOW': 'http://maps.google.com/mapfiles/kml/paddle/red-circle.png',
        }
        placemark.style.iconstyle.icon.href = icon_colors.get(site.confidence_level, icon_colors['LOW'])
        
        # Rich description
        tools_str = ', '.join(site.tools_used) if site.tools_used else 'Unknown'
        
        placemark.description = f"""
        <h2>{site.grid_ref}</h2>
        
        <p><b>Confidence:</b> {site.confidence_score*100:.1f}% ({site.confidence_level})</p>
        <p><b>Classification:</b> {site.classification}</p>
        <p><b>Detections:</b> {site.total_detections}x</p>
        
        <h3>Details</h3>
        <table>
            <tr><td>Tools:</td><td>{tools_str}</td></tr>
            <tr><td>Material:</td><td>{site.material}</td></tr>
            <tr><td>BBOX:</td><td>{site.bbox_id or 'Global'}</td></tr>
            <tr><td>First Detected:</td><td>{site.first_detected or 'Unknown'}</td></tr>
            <tr><td>Last Detected:</td><td>{site.last_detected or 'Unknown'}</td></tr>
        </table>
        
        {'<p>⚠️ Debris Field (scattered detections)</p>' if site.is_debris_field else ''}
        {'<p>✅ Identified: ' + site.wreck_name + '</p>' if site.identified and site.wreck_name else ''}
        <p><b>Release Status:</b> {site.release_status}</p>
        """
    
    # Save to bytes
    import io
    buffer = io.BytesIO()
    kml.savekmz(buffer)
    return buffer.getvalue()

# ============================================================================
# WEB UI ROUTES
# ============================================================================

@app.route('/')
def index():
    """Home page"""
    return render_template('index.html')

@app.route('/sorter')
def sorter_ui():
    """Sorter control panel"""
    return render_template('sorter.html')

@app.route('/feed.kmz')
def public_feed():
    """
    Public KMZ feed for Google Earth
    Only shows CONFIRMED and PUBLIC sites
    """
    try:
        with DetectionSorter(DB_PATH, admin_mode=False) as sorter:
            sites = sorter.apply()
            kmz_data = generate_kmz(sites, "CESAROPS Public Feed")
            
            response = make_response(kmz_data)
            response.headers['Content-Type'] = 'application/vnd.google-earth.kmz'
            response.headers['Cache-Control'] = 'no-cache, max-age=0'
            return response
    except Exception as e:
        return jsonify({'error': str(e)}), 500

# ============================================================================
# API ROUTES
# ============================================================================

@app.route('/api/login', methods=['POST'])
def api_login():
    """
    Login with API key
    
    POST /api/login
    {
        "api_key": "cesarops_admin_key_2026"
    }
    """
    data = request.json
    api_key = data.get('api_key')
    
    if api_key in API_KEYS:
        session['api_key'] = api_key
        permissions = API_KEYS[api_key]
        
        return jsonify({
            'success': True,
            'permissions': permissions,
            'api_key': api_key[:10] + '...'  # Show partial key for confirmation
        })
    
    return jsonify({'error': 'Invalid API key'}), 401

@app.route('/api/logout', methods=['POST'])
def api_logout():
    """Logout"""
    session.pop('api_key', None)
    return jsonify({'success': True})

@app.route('/api/sorter', methods=['POST'])
def api_sorter():
    """
    Apply filters and get results
    
    POST /api/sorter
    {
        "filters": {
            "min_confidence": 0.8,
            "tools": ["M2200", "P1000"],
            "release_status": ["CONFIRMED", "PUBLIC"],
            "bbox": "MICHIGAN_SOUTH",
            "material": ["steel"]
        }
    }
    """
    
    # Check authentication
    api_key = request.headers.get('X-API-Key') or session.get('api_key')
    
    if not api_key or api_key not in API_KEYS:
        return jsonify({'error': 'Invalid API key'}), 401
    
    data = request.json
    filters = data.get('filters', {})
    
    # Determine admin mode
    admin_mode = api_key == "cesarops_admin_key_2026"
    
    try:
        with DetectionSorter(DB_PATH, admin_mode=admin_mode, api_key=api_key) as sorter:
            # Apply filters
            for key, value in filters.items():
                sorter.set_filter(key, value)
            
            # Get results
            sites = sorter.apply()
            stats = sorter.get_stats()
            
            return jsonify({
                'success': True,
                'total_sites': len(sites),
                'stats': stats,
                'sites': [site.to_dict() for site in sites]
            })
    
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/site/<int:site_id>/release', methods=['POST'])
def api_toggle_release(site_id):
    """
    Change release status of a site (admin only)
    
    POST /api/site/42/release
    {
        "status": "PUBLIC"  // INTERNAL, CONFIRMED, PUBLIC, HIDDEN
    }
    """
    
    # Check authentication
    api_key = request.headers.get('X-API-Key') or session.get('api_key')
    
    if not api_key or api_key not in API_KEYS:
        return jsonify({'error': 'Invalid API key'}), 401
    
    # Check admin permission
    if 'admin' not in API_KEYS.get(api_key, []):
        return jsonify({'error': 'Admin access required'}), 403
    
    data = request.json
    new_status = data.get('status')
    
    if new_status not in ['INTERNAL', 'CONFIRMED', 'PUBLIC', 'HIDDEN']:
        return jsonify({'error': 'Invalid status'}), 400
    
    try:
        conn = sqlite3.connect(str(DB_PATH))
        cursor = conn.cursor()
        cursor.execute('''
            UPDATE detection_sites
            SET release_status = ?, released_at = ?, released_by = ?
            WHERE site_id = ?
        ''', (new_status, datetime.now().isoformat(), f'api:{api_key[:10]}', site_id))
        conn.commit()
        
        updated = cursor.rowcount > 0
        conn.close()
        
        if updated:
            return jsonify({
                'success': True,
                'site_id': site_id,
                'new_status': new_status
            })
        else:
            return jsonify({'error': 'Site not found'}), 404
    
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/export.kmz', methods=['POST'])
def api_export_kmz():
    """
    Generate custom KMZ with current filters
    
    POST /api/export.kmz
    {
        "filters": {...}
    }
    """
    
    # Check authentication
    api_key = request.headers.get('X-API-Key') or session.get('api_key')
    
    if not api_key or api_key not in API_KEYS:
        return jsonify({'error': 'Invalid API key'}), 401
    
    # Check export permission
    if 'export' not in API_KEYS.get(api_key, []):
        return jsonify({'error': 'Export permission required'}), 403
    
    data = request.json
    filters = data.get('filters', {})
    
    try:
        admin_mode = api_key == "cesarops_admin_key_2026"
        
        with DetectionSorter(DB_PATH, admin_mode=admin_mode, api_key=api_key) as sorter:
            for key, value in filters.items():
                sorter.set_filter(key, value)
            
            sites = sorter.apply()
            kmz_data = generate_kmz(sites, "CESAROPS Export")
            
            response = make_response(kmz_data)
            response.headers['Content-Type'] = 'application/vnd.google-earth.kmz'
            response.headers['Content-Disposition'] = 'attachment; filename=cesarops_export.kmz'
            return response
    
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/stats')
def api_stats():
    """Get current database statistics"""
    
    # Check authentication
    api_key = request.headers.get('X-API-Key') or session.get('api_key')
    
    if not api_key or api_key not in API_KEYS:
        return jsonify({'error': 'Invalid API key'}), 401
    
    try:
        with DetectionSorter(DB_PATH, admin_mode=True, api_key=api_key) as sorter:
            stats = sorter.get_stats()
            return jsonify({'success': True, 'stats': stats})
    except Exception as e:
        return jsonify({'error': str(e)}), 500

# ============================================================================
# MAIN
# ============================================================================

def main():
    print("="*70)
    print("CESAROPS LIVE FEED SERVER")
    print("="*70)
    print()
    print(f"Database: {DB_PATH.absolute()}")
    print()
    print("Access URLs:")
    print("  Web UI:     http://localhost:8080/sorter")
    print("  Public KMZ: http://localhost:8080/feed.kmz")
    print("  API:        http://localhost:8080/api/sorter")
    print()
    print("API Keys:")
    for key, perms in API_KEYS.items():
        print(f"  {key[:15]}... - {', '.join(perms)}")
    print()
    print("="*70)
    
    # Check if database exists
    if not DB_PATH.exists():
        print(f"\n⚠️  Database not found: {DB_PATH}")
        print("Run cesarops_engine.py first to create database")
        print()
    
    # Run server
    app.run(host='0.0.0.0', port=8080, debug=False)

if __name__ == '__main__':
    main()
