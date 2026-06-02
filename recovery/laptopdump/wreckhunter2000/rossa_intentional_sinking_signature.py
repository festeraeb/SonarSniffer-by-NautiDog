"""
rossa_intentional_sinking_signature.py

Search signatures for INTENTIONAL vessel sinking (Rossa).

Vessel: 1970s-80s Bristol 32 sailboat
- Diesel engine (or Atomic 4 gasoline)
- 2-4 solar panels on stern
- Lead keel (~2 tons)
- Closed hatches, opened window, pulled through-hull

Expected Signatures:
1. OIL SLICK (diesel/gas leak from tank)
2. SOLAR PANELS (metal reflectance in NIR/SWIR)
3. INTACT HULL (not scattered debris)
4. THERMAL SINK (lead keel + engine block = cold mass)
5. NO DEBRIS FIELD (everything stowed below)

Search Priority:
1. Sentinel-1 SAR (oil slick detection)
2. Sentinel-2 NIR/SWIR (solar panel reflectance)
3. Landsat TIRS (thermal mass from lead keel)
4. SWOT (height anomaly from intact mast/hull)
"""

import json
from pathlib import Path
from datetime import datetime

# ── Rossa Specifications ─────────────────────────────────────────────────────

ROSSA_SPECS = {
    'vessel_type': 'Bristol 32 sailboat',
    'year': '1970s-1980s',
    'length_loa': '32 feet (9.75 m)',
    'beam': '10.5 feet (3.2 m)',
    'draft': '5.5 feet (1.68 m)',
    'displacement': '~11,000 lbs (5,000 kg)',
    'keel': 'Lead, ~2,000 lbs (900 kg)',
    'engine': 'Diesel (or Atomic 4 gasoline)',
    'fuel_tank': '~20 gallons diesel/gas',
    'solar_panels': '2-4 panels on stern (metal reflectance)',
    'construction': 'Fiberglass hull (FRP)',
}

# ── Expected Signatures ──────────────────────────────────────────────────────

SIGNATURES = {
    'oil_slick': {
        'description': 'Light diesel/gasoline slick from fuel tank leak',
        'best_sensor': 'Sentinel-1 SAR (C-band)',
        'signature': 'Dark patch in SAR (smooths surface texture)',
        'size': '~50-200 m diameter (small, dissipates in 1-3 days)',
        'priority': 'HIGH',
    },
    'solar_panels': {
        'description': '2-4 solar panels on stern (glass/metal)',
        'best_sensor': 'Sentinel-2 NIR/SWIR (B11, B12)',
        'signature': 'High reflectance in SWIR (metal/glass signature)',
        'size': '~2x4 meters total (sub-pixel in 10m bands)',
        'priority': 'MEDIUM',
    },
    'intact_hull': {
        'description': 'Vessel intact, upright, on keel (not scattered)',
        'best_sensor': 'Sentinel-2 optical + SAR',
        'signature': 'Compact 10x3m anomaly (not debris scatter)',
        'size': '~10m x 3m (hull dimensions)',
        'priority': 'CRITICAL',
    },
    'thermal_sink': {
        'description': 'Lead keel + engine block = cold mass',
        'best_sensor': 'Landsat 8/9 TIRS (B10/B11)',
        'signature': 'Negative Z-score (cold spot in warm water)',
        'size': '~3-5m diameter (keel footprint)',
        'priority': 'CRITICAL',
    },
    'height_anomaly': {
        'description': 'Mast/cabin creates height above lakebed',
        'best_sensor': 'SWOT SSH, ICESat-2 ATL13',
        'signature': '>1cm height anomaly (mast or cabin top)',
        'size': '~2-5m vertical (if mast still attached)',
        'priority': 'MEDIUM',
    },
    'no_debris_field': {
        'description': 'NO scattering of debris (everything stowed)',
        'best_sensor': 'All sensors',
        'signature': 'ABSENCE of debris field around wreck site',
        'size': 'N/A',
        'priority': 'NOTE',
    },
}

# ── Search Strategy ───────────────────────────────────────────────────────────

SEARCH_STRATEGY = {
    'phase_1': {
        'name': 'OIL SLICK DETECTION (Days 1-3)',
        'sensors': ['Sentinel-1 SAR'],
        'dates': 'Aug 22-25, 2025',
        'what_to_look_for': 'Dark patches in SAR imagery (smooth surface)',
        'notes': 'Oil slick dissipates quickly - only visible first 1-3 days',
    },
    'phase_2': {
        'name': 'INTACT HULL DETECTION (Days 3-14)',
        'sensors': ['Sentinel-2 optical', 'Sentinel-1 SAR'],
        'dates': 'Aug 25 - Sep 5, 2025',
        'what_to_look_for': 'Compact 10x3m anomaly (not scattered debris)',
        'notes': 'Look for vessel-shaped signature, not debris field',
    },
    'phase_3': {
        'name': 'THERMAL MASS DETECTION (Ongoing)',
        'sensors': ['Landsat 8/9 TIRS'],
        'dates': 'Aug 22 - present',
        'what_to_look_for': 'Cold spot (negative Z-score) in thermal imagery',
        'notes': 'Lead keel stays at 4°C - persistent thermal signature',
    },
    'phase_4': {
        'name': 'SOLAR PANEL REFLECTANCE (Clear water only)',
        'sensors': ['Sentinel-2 SWIR (B11, B12)'],
        'dates': 'Aug 22 - present',
        'what_to_look_for': 'High SWIR reflectance (metal/glass signature)',
        'notes': 'Sub-pixel detection - look for anomalous reflectance',
    },
}

# ── Output ────────────────────────────────────────────────────────────────────

OUTPUT_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/rossa_intentional_sinking')
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)


def generate_search_guide():
    """Generate comprehensive search guide for intentional sinking."""
    
    guide = {
        'vessel': ROSSA_SPECS,
        'signatures': SIGNATURES,
        'search_strategy': SEARCH_STRATEGY,
        'generated_at': datetime.now().isoformat(),
        'notes': [
            'NO debris field expected - everything stowed below',
            'Fender found separately - drifted 76 NM to Grand Haven',
            'Likely intentional: closed hatches, opened window, pulled through-hull',
            'Search for INTACT vessel, not scattered wreckage',
            'Oil slick only visible first 1-3 days (Aug 22-25)',
            'Thermal signature from lead keel is PERSISTENT (years)',
        ],
    }
    
    # Save JSON
    json_path = OUTPUT_DIR / 'rossa_intentional_sinking_guide.json'
    with open(json_path, 'w') as f:
        json.dump(guide, f, indent=2)
    
    # Print summary
    print('='*70)
    print('ROSSA INTENTIONAL SINKING - SEARCH GUIDE')
    print('='*70)
    print()
    print('VESSEL:')
    print(f'  {ROSSA_SPECS["vessel_type"]} ({ROSSA_SPECS["year"]})')
    print(f'  Length: {ROSSA_SPECS["length_loa"]}')
    print(f'  Keel: {ROSSA_SPECS["keel"]}')
    print(f'  Engine: {ROSSA_SPECS["engine"]}')
    print(f'  Solar: {ROSSA_SPECS["solar_panels"]}')
    print()
    print('EXPECTED SIGNATURES:')
    for sig_name, sig_data in SIGNATURES.items():
        print(f'  [{sig_data["priority"]}] {sig_name.upper()}')
        print(f'       Sensor: {sig_data["best_sensor"]}')
        print(f'       {sig_data["description"]}')
        print()
    print('SEARCH STRATEGY:')
    for phase_name, phase_data in SEARCH_STRATEGY.items():
        print(f'  {phase_data["name"]}')
        print(f'       Dates: {phase_data["dates"]}')
        print(f'       Sensors: {", ".join(phase_data["sensors"])}')
        print(f'       Look for: {phase_data["what_to_look_for"]}')
        print()
    print('='*70)
    print(f'Guide saved: {json_path}')
    print('='*70)
    
    return guide


if __name__ == '__main__':
    generate_search_guide()
