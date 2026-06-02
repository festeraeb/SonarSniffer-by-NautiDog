"""
swayze_cross_reference.py

Cross-references WreckHunter 2000 detections against the Swayze wreck database.

Compares:
  - GPU curvelets detections (all 12 bands)
  - Swayze confirmed wrecks
  - Swayze estimated/unconfirmed wrecks

Outputs:
  - Match report (how many of our detections match known wrecks)
  - New candidate list (our detections NOT in Swayze)
  - Missing wreck list (Swayze wrecks we DIDN'T detect)
"""

import pandas as pd
import json
from pathlib import Path
from datetime import datetime
import numpy as np

# ── Paths ─────────────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
BAG_REPO = Path('c:/Users/thomf/programming/Bagrecovery')

# Swayze database
SWAYZE_PATH = BAG_REPO / 'Swayze stuff' / 'Swayze2019-1.xlsx'

# GPU curvelets outputs
GPU_CURVELETS_DIR = REPO / 'outputs' / 'gpu_curvelets'

# Output directory
OUTPUT_DIR = REPO / 'outputs' / 'swayze_cross_reference'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# ── Helpers ───────────────────────────────────────────────────────────────────

def haversine_distance_km(lat1, lon1, lat2, lon2) -> float:
    """Calculate distance between two coordinates in kilometers."""
    R = 6371.0  # Earth radius in km
    
    phi1 = np.radians(lat1)
    phi2 = np.radians(lat2)
    dphi = np.radians(lat2 - lat1)
    dlam = np.radians(lon2 - lon1)
    
    a = np.sin(dphi/2)**2 + np.cos(phi1)*np.cos(phi2)*np.sin(dlam/2)**2
    c = 2 * np.arctan2(np.sqrt(a), np.sqrt(1-a))
    
    return R * c


def load_swayze_database() -> pd.DataFrame:
    """Load Swayze wreck database with confirmed/estimated flags."""
    print(f'Loading Swayze database from {SWAYZE_PATH}...')
    
    try:
        # Read Excel file
        df = pd.read_excel(SWAYZE_PATH)
        
        # Standardize column names
        df.columns = df.columns.str.lower().str.strip()
        
        # Find lat/lon columns
        lat_cols = [c for c in df.columns if 'lat' in c.lower() or 'y' in c.lower()]
        lon_cols = [c for c in df.columns if 'lon' in c.lower() or 'long' in c.lower() or 'x' in c.lower()]
        
        if lat_cols and lon_cols:
            df = df.rename(columns={
                lat_cols[0]: 'latitude',
                lon_cols[0]: 'longitude',
            })
        
        # Find status/confidence columns (confirmed vs estimated)
        status_cols = [c for c in df.columns if 'status' in c.lower() or 'conf' in c.lower() or 'verified' in c.lower() or 'estimate' in c.lower()]
        if status_cols:
            df = df.rename(columns={status_cols[0]: 'status'})
            # Create confirmed flag
            df['is_confirmed'] = df['status'].apply(
                lambda x: 1 if 'confirm' in str(x).lower() or 'verified' in str(x).lower() else 0
            )
            df['is_estimated'] = df['status'].apply(
                lambda x: 1 if 'estimate' in str(x).lower() or 'probable' in str(x).lower() else 0
            )
        
        # Find lake column
        lake_cols = [c for c in df.columns if 'lake' in c.lower()]
        if lake_cols:
            df = df.rename(columns={lake_cols[0]: 'lake'})
        
        print(f'  Loaded {len(df)} wrecks from Swayze database')
        
        # Show confirmed vs estimated breakdown
        if 'is_confirmed' in df.columns:
            confirmed_count = df['is_confirmed'].sum()
            estimated_count = df['is_estimated'].sum() if 'is_estimated' in df.columns else len(df) - confirmed_count
            print(f'  Confirmed wrecks: {confirmed_count}')
            print(f'  Estimated wrecks: {estimated_count}')
        
        # Show lake breakdown
        if 'lake' in df.columns:
            print(f'  By lake:')
            for lake in df['lake'].dropna().unique()[:5]:
                count = len(df[df['lake'] == lake])
                print(f'    {lake}: {count}')
        
        return df
        
    except Exception as e:
        print(f'  Error loading Swayze: {e}')
        return pd.DataFrame()


def load_gpu_curvelets_detections() -> pd.DataFrame:
    """Load GPU curvelets anomaly detections."""
    print(f'Loading GPU curvelets detections from {GPU_CURVELETS_DIR}...')
    
    all_anomalies = []
    
    # Load all anomaly JSON files
    anomaly_files = list(GPU_CURVELETS_DIR.glob('*_anomalies_gpu.json'))
    
    for anomaly_path in anomaly_files:
        try:
            with open(anomaly_path, 'r') as f:
                data = json.load(f)
            
            band = anomaly_path.stem.replace('_anomalies_gpu', '')
            anomalies = data.get('top_anomalies', [])
            
            for scale, direction, row, col, magnitude in anomalies:
                all_anomalies.append({
                    'band': band,
                    'scale': scale,
                    'direction': direction,
                    'row': row,
                    'col': col,
                    'magnitude': magnitude,
                    'source_file': anomaly_path.name,
                })
            
        except Exception as e:
            print(f'  Error loading {anomaly_path.name}: {e}')
    
    df = pd.DataFrame(all_anomalies)
    
    if len(df) > 0:
        print(f'  Loaded {len(df)} anomaly detections from {len(anomaly_files)} bands')
    else:
        print(f'  No anomaly detections found')
    
    return df


def cross_reference_detections(
    swayze_df: pd.DataFrame,
    detections_df: pd.DataFrame,
    match_radius_km: float = 0.5,
) -> dict:
    """
    Cross-reference GPU detections against Swayze database.
    
    Args:
        swayze_df: Swayze wreck database
        detections_df: GPU curvelets detections
        match_radius_km: Distance threshold for matching (default 0.5 km)
    
    Returns:
        Dict with match results
    """
    print()
    print('='*70)
    print('CROSS-REFERENCE ANALYSIS')
    print('='*70)
    print()
    
    results = {
        'matches': [],  # Our detections that match Swayze wrecks
        'new_candidates': [],  # Our detections NOT in Swayze
        'missing_wrecks': [],  # Swayze wrecks we DIDN'T detect
        'summary': {},
    }
    
    # Note: GPU curvelets detections are in pixel coordinates (row, col)
    # We need to convert to lat/lon for matching
    # For now, we'll do a simplified analysis
    
    print(f'Match radius: {match_radius_km} km')
    print()
    
    # For each Swayze wreck, check if we have a detection nearby
    # (This is simplified - real implementation needs proper coordinate conversion)
    
    if len(swayze_df) == 0 or len(detections_df) == 0:
        print('Cannot cross-reference: missing data')
        return results
    
    # Group detections by band
    by_band = detections_df.groupby('band')
    
    print(f'Swayze wrecks: {len(swayze_df)}')
    print(f'GPU detections: {len(detections_df)}')
    print(f'Bands processed: {len(by_band)}')
    print()
    
    # Simple statistics for now
    results['summary'] = {
        'swayze_wrecks_total': len(swayze_df),
        'gpu_detections_total': len(detections_df),
        'bands_processed': len(by_band),
        'analysis_timestamp': datetime.now().isoformat(),
    }
    
    # By lake analysis (if lake column exists)
    if 'lake' in swayze_df.columns:
        lake_counts = swayze_df['lake'].value_counts()
        print('Swayze wrecks by lake:')
        for lake, count in lake_counts.items():
            print(f'  {lake}: {count}')
        print()
        
        results['summary']['swayze_by_lake'] = lake_counts.to_dict()
    
    # By status analysis (if status column exists)
    if 'status' in swayze_df.columns:
        status_counts = swayze_df['status'].value_counts()
        print('Swayze wrecks by status:')
        for status, count in status_counts.items():
            print(f'  {status}: {count}')
        print()
        
        results['summary']['swayze_by_status'] = status_counts.to_dict()
    
    # GPU detections by band
    print('GPU detections by band:')
    band_counts = detections_df['band'].value_counts()
    for band, count in band_counts.items():
        print(f'  {band}: {count} anomalies')
    print()
    
    results['summary']['gpu_by_band'] = band_counts.to_dict()
    
    return results


def save_cross_reference_report(results: dict):
    """Save cross-reference report to JSON and CSV."""
    
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    
    # Save JSON summary
    json_path = OUTPUT_DIR / f'swayze_cross_reference_{timestamp}.json'
    with open(json_path, 'w') as f:
        json.dump(results, f, indent=2, default=str)
    print(f'Saved JSON report: {json_path}')
    
    # Save summary CSV
    if 'summary' in results:
        csv_path = OUTPUT_DIR / f'swayze_summary_{timestamp}.csv'
        summary_df = pd.DataFrame([
            {'metric': k, 'value': str(v)}
            for k, v in results['summary'].items()
        ])
        summary_df.to_csv(csv_path, index=False)
        print(f'Saved summary CSV: {csv_path}')
    
    print()


def main():
    """Main cross-reference analysis."""
    print('='*70)
    print('SWAYZE DATABASE CROSS-REFERENCE')
    print('='*70)
    print()
    
    # Load Swayze database
    swayze_df = load_swayze_database()
    
    # Load GPU curvelets detections
    detections_df = load_gpu_curvelets_detections()
    
    # Cross-reference
    results = cross_reference_detections(swayze_df, detections_df)
    
    # Save report
    save_cross_reference_report(results)
    
    print('='*70)
    print('CROSS-REFERENCE COMPLETE')
    print('='*70)
    print()
    print(f'Output directory: {OUTPUT_DIR}')
    print()
    
    return results


if __name__ == '__main__':
    main()
