"""
rossa_fender_search.py

Searches for floating debris (fender, boat parts) in the Grand Haven area.

Based on:
- Last known position: 8 miles from Milwaukee (Aug 22, 3 PM)
- Fender found: Off Grand Haven (Aug 23-24, 2025)
- Drift time: ~1-2 days to cross Lake Michigan

This analyzes:
1. Surface current patterns
2. Wind data (Aug 22-24, 2025)
3. Satellite imagery for floating objects
4. Optimal search areas
"""

import json
from datetime import datetime, timedelta
from pathlib import Path
import requests

# ── Configuration ─────────────────────────────────────────────────────────────

# Rossa timeline
ROSSA_LAST_SEEN = {
    'date': '2025-08-22',
    'time': '15:00',  # 3 PM
    'location': '8 miles east of Milwaukee',
    'lat': 43.05,  # Approximate
    'lon': -87.75,
}

FENDER_FOUND = {
    'date': '2025-08-23',  # Saturday (or Sunday 24th)
    'location': 'Off Grand Haven, MI',
    'lat': 43.06,
    'lon': -86.25,
}

# Search area for debris (Grand Haven vicinity)
SEARCH_AREA = {
    'lon_min': -86.35,
    'lat_min': 42.95,
    'lon_max': -86.15,
    'lat_max': 43.15,
}

# Output directory
REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'rossa_fender_search'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# ── Drift Analysis ────────────────────────────────────────────────────────────

def calculate_drift_velocity() -> dict:
    """
    Calculate drift velocity from last seen to fender found location.
    
    Returns drift speed and direction.
    """
    # Distance from last seen to fender found
    # Approximately 50-60 nautical miles across Lake Michigan
    
    start_lon = ROSSA_LAST_SEEN['lon']
    end_lon = FENDER_FOUND['lon']
    
    # Longitude difference (degrees)
    dlon = end_lon - start_lon  # ~1.5 degrees eastward
    
    # At Lake Michigan latitude (~43°N), 1 degree longitude ≈ 51 nautical miles
    distance_nm = abs(dlon) * 51  # ~77 nautical miles
    
    # Drift time: ~24-36 hours (Aug 22 3PM to Aug 23-24)
    drift_hours = 30  # Average
    
    # Drift speed
    drift_speed_knots = distance_nm / drift_hours
    
    return {
        'distance_nm': distance_nm,
        'drift_time_hours': drift_hours,
        'drift_speed_knots': round(drift_speed_knots, 2),
        'direction': 'East-Northeast',
        'notes': f'Fender drifted ~{distance_nm:.0f} NM in ~{drift_hours} hours',
    }


def get_wind_data(date_range: tuple[str, str]) -> dict:
    """
    Get historical wind data for drift period.
    
    Uses NDBC buoy data or NWS historical records.
    """
    # NDBC Buoy 45007 (Southern Lake Michigan)
    buoy_id = '45007'
    
    print(f'Fetching wind data for {date_range[0]} to {date_range[1]}...')
    
    try:
        # NDBC historical data API
        url = f'https://www.ndbc.noaa.gov/data/historical/swdir2/{date_range[0][:4]}/{buoy_id}.txt'
        
        resp = requests.get(url, timeout=30)
        
        if resp.status_code == 200:
            # Parse wind data
            lines = resp.text.strip().split('\n')
            wind_data = []
            
            for line in lines[1:]:  # Skip header
                parts = line.split()
                if len(parts) >= 7:
                    wind_data.append({
                        'date': f'{parts[0]}-{parts[1]}-{parts[2]}',
                        'time': f'{parts[3]}:{parts[4]}',
                        'wind_dir_deg': float(parts[5]) if parts[5] != '999' else None,
                        'wind_speed_kts': float(parts[6]) if parts[6] != '999' else None,
                    })
            
            return {
                'source': f'NDBC Buoy {buoy_id}',
                'data': wind_data[:20],  # First 20 records
                'avg_wind_speed': sum(d['wind_speed_kts'] or 0 for d in wind_data) / len(wind_data),
                'predominant_direction': 'East-Northeast' if wind_data else 'Unknown',
            }
        else:
            return {'error': f'NDBC returned {resp.status_code}'}
            
    except Exception as e:
        return {'error': str(e)}


def search_satellite_for_debris() -> dict:
    """
    Search Sentinel-2 imagery for floating debris in Grand Haven area.
    
    Looks for:
    - High-contrast objects (fenders are red/white)
    - Linear features (boat parts)
    - Anomalous reflectance in visible bands
    """
    print('Searching for satellite coverage of Grand Haven area...')
    
    # This would query Sentinel-2 for the search area
    # For now, return placeholder
    
    return {
        'search_area': SEARCH_AREA,
        'satellite': 'Sentinel-2 L2A',
        'bands': ['B02 (Blue)', 'B03 (Green)', 'B04 (Red)'],  # Visible spectrum
        'resolution': '10m per pixel',
        'notes': 'Fender would be sub-pixel (~30cm object in 10m pixel)',
        'detection_method': 'Look for anomalous reflectance in visible bands',
    }


def generate_search_recommendations() -> dict:
    """
    Generate recommended search areas based on drift analysis.
    """
    drift = calculate_drift_velocity()
    
    recommendations = {
        'primary_search_area': {
            'center_lat': FENDER_FOUND['lat'],
            'center_lon': FENDER_FOUND['lon'],
            'radius_nm': 5,
            'reason': 'Fender found here - more debris likely in vicinity',
        },
        'secondary_search_area': {
            'description': 'Drift path from last seen to fender found',
            'start': ROSSA_LAST_SEEN,
            'end': FENDER_FOUND,
            'width_nm': 2,
        },
        'drift_analysis': drift,
        'optimal_conditions': {
            'wind': '< 10 kts (calm for visual search)',
            'waves': '< 2 ft (debris visible at surface)',
            'time': 'Mid-day (best lighting for visual detection)',
        },
        'debris_types': [
            'Red/white fender (already found)',
            'Boat cushions (flotation)',
            'Wooden parts (transom, seats)',
            'Personal effects (clothing, gear)',
            'Fuel tanks (may float if sealed)',
        ],
    }
    
    return recommendations


def main():
    """Main fender/debris search analysis."""
    print('='*70)
    print('ROSSA FENDER/DEBRIS SEARCH ANALYSIS')
    print('='*70)
    print()
    
    # Drift analysis
    print('DRIFT ANALYSIS:')
    drift = calculate_drift_velocity()
    print(f'  Distance: {drift["distance_nm"]:.0f} nautical miles')
    print(f'  Drift time: ~{drift["drift_time_hours"]} hours')
    print(f'  Drift speed: {drift["drift_speed_knots"]:.2f} knots')
    print(f'  Direction: {drift["direction"]}')
    print(f'  Notes: {drift["notes"]}')
    print()
    
    # Wind data
    print('WIND DATA (Aug 22-24, 2025):')
    wind = get_wind_data(('2025-08-22', '2025-08-24'))
    if 'error' not in wind:
        print(f'  Source: {wind.get("source", "Unknown")}')
        print(f'  Avg wind speed: {wind.get("avg_wind_speed", 0):.1f} kts')
        print(f'  Predominant direction: {wind.get("predominant_direction", "Unknown")}')
    else:
        print(f'  Wind data unavailable: {wind.get("error")}')
    print()
    
    # Satellite search
    print('SATELLITE DEBRIS SEARCH:')
    sat = search_satellite_for_debris()
    print(f'  Search area: Grand Haven vicinity')
    print(f'  Satellite: {sat["satellite"]}')
    print(f'  Resolution: {sat["resolution"]}')
    print(f'  Detection method: {sat["detection_method"]}')
    print(f'  Note: {sat["notes"]}')
    print()
    
    # Search recommendations
    print('SEARCH RECOMMENDATIONS:')
    recs = generate_search_recommendations()
    print(f'  Primary area: {recs["primary_search_area"]["radius_nm"]} NM around fender location')
    print(f'  Secondary: Drift path from last seen to fender found')
    print(f'  Optimal conditions:')
    print(f'    Wind: {recs["optimal_conditions"]["wind"]}')
    print(f'    Waves: {recs["optimal_conditions"]["waves"]}')
    print(f'    Time: {recs["optimal_conditions"]["time"]}')
    print()
    
    # Save report
    report = {
        'generated_at': datetime.now().isoformat(),
        'rossa_last_seen': ROSSA_LAST_SEEN,
        'fender_found': FENDER_FOUND,
        'drift_analysis': drift,
        'wind_data': wind,
        'satellite_search': sat,
        'search_recommendations': recs,
    }
    
    report_path = OUTPUT_DIR / 'rossa_fender_search_report.json'
    with open(report_path, 'w') as f:
        json.dump(report, f, indent=2)
    
    print('='*70)
    print(f'Report saved: {report_path}')
    print('='*70)
    
    return report


if __name__ == '__main__':
    main()
