#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
CESAROPS Smart Daily Scan - Temporal Sweep Strategy

Strategy:
1. Use lake bounding boxes to filter out land
2. Sweep same date across multiple years (May 15 - Dec 15)
3. Find repeatable anomalies NOT in database
4. Flag for deep analysis with special tools
5. Offload files at end of day
6. Move to next date

Usage:
    python smart_daily_scan.py --date "05-15" --years 2015-2025
"""

import sys
import json
from pathlib import Path
from datetime import datetime, timedelta
from typing import List, Dict, Tuple

# Import sorter for DB comparison
from detection_sorter import DetectionSorter

# ============================================================================
# CONFIGURATION
# ============================================================================

# Lake bounding boxes (filter out land)
LAKE_BBOXES = {
    'MICHIGAN': {
        'name': 'Lake Michigan',
        'bbox': (42.4, -87.5, 45.5, -85.5),  # (min_lat, min_lon, max_lat, max_lon)
        'priority': 1,
        'scan_order': 1,
    },
    'ERIE': {
        'name': 'Lake Erie',
        'bbox': (41.5, -83.5, 42.5, -80.5),
        'priority': 2,
        'scan_order': 2,
    },
    'HURON': {
        'name': 'Lake Huron',
        'bbox': (43.5, -83.5, 45.5, -81.5),
        'priority': 3,
        'scan_order': 3,
    },
    'SUPERIOR': {
        'name': 'Lake Superior',
        'bbox': (46.5, -92.0, 48.0, -84.0),
        'priority': 4,
        'scan_order': 4,
    },
    'ONTARIO': {
        'name': 'Lake Ontario',
        'bbox': (43.5, -77.5, 44.5, -76.0),
        'priority': 5,
        'scan_order': 5,
    },
}

# Scan season (May 15 - Dec 15)
SCAN_SEASON_START = (5, 15)   # May 15
SCAN_SEASON_END = (12, 15)    # Dec 15

# Years to sweep
SWEEP_YEARS = list(range(2015, 2026))  # 2015-2025

# Repeatable threshold (appears in N+ years)
REPEATABLE_THRESHOLD = 2

# Paths
DB_PATH = Path(__file__).parent / "wreckhunter2000" / "LAKE_MICHIGAN_CENSUS_2026.db"
DATA_DIR = Path(__file__).parent / "wreckhunter2000" / "data"
OFFLOAD_DIR = Path(__file__).parent / "outputs" / "offload"
DEEP_ANALYSIS_DIR = Path(__file__).parent / "outputs" / "deep_analysis"

# Ensure directories exist
OFFLOAD_DIR.mkdir(parents=True, exist_ok=True)
DEEP_ANALYSIS_DIR.mkdir(parents=True, exist_ok=True)

# ============================================================================
# DATE SWEEP SCHEDULER
# ============================================================================

class DateSweepScheduler:
    """Manages temporal sweep across years for each date"""
    
    def __init__(self, start_date: Tuple[int, int], end_date: Tuple[int, int], years: List[int]):
        self.start_month, self.start_day = start_date
        self.end_month, self.end_day = end_date
        self.years = years
        
        # Generate all dates to sweep
        self.dates_to_sweep = self._generate_dates()
        self.current_index = 0
    
    def _generate_dates(self) -> List[datetime]:
        """Generate all dates in scan season"""
        dates = []
        
        # Start from year 2015 (doesn't matter which year, we just need month/day)
        current = datetime(2015, self.start_month, self.start_day)
        end = datetime(2015, self.end_month, self.end_day)
        
        while current <= end:
            dates.append(current)
            current += timedelta(days=1)
        
        return dates
    
    def get_current_date(self) -> datetime:
        """Get current date in sweep"""
        if self.current_index >= len(self.dates_to_sweep):
            return None  # Completed full sweep
        return self.dates_to_sweep[self.current_index]
    
    def get_sweep_dates(self) -> List[datetime]:
        """Get all years for current date"""
        current = self.get_current_date()
        if not current:
            return []
        
        # Generate same date for all years
        sweep_dates = []
        for year in self.years:
            try:
                sweep_dates.append(datetime(year, current.month, current.day))
            except ValueError:
                # Skip Feb 29 for non-leap years
                continue
        
        return sweep_dates
    
    def advance(self):
        """Move to next date"""
        self.current_index += 1
    
    def get_progress(self) -> Dict:
        """Get sweep progress"""
        total = len(self.dates_to_sweep)
        current = self.current_index
        remaining = total - current
        
        return {
            'current_date': self.get_current_date().strftime('%m-%d') if self.get_current_date() else 'Complete',
            'dates_completed': current,
            'dates_remaining': remaining,
            'total_dates': total,
            'percent_complete': (current / total * 100) if total > 0 else 100,
        }

# ============================================================================
# REPEATABLE DETECTION FINDER
# ============================================================================

class RepeatableDetectionFinder:
    """Find anomalies that appear across multiple years"""
    
    def __init__(self, db_path: Path):
        self.db_path = db_path
    
    def find_repeatable_anomalies(self, sweep_dates: List[datetime], lake_bbox: Tuple) -> List[Dict]:
        """
        Find anomalies that appear in multiple years for same date range
        
        Args:
            sweep_dates: List of dates (same month/day, different years)
            lake_bbox: (min_lat, min_lon, max_lat, max_lon)
        
        Returns:
            List of repeatable anomalies not in DB
        """
        
        # Group by spatial location (within 50m tolerance)
        anomaly_clusters = {}
        
        for date in sweep_dates:
            # In production, this would query satellite data for this date
            # For now, simulate with mock data
            anomalies = self._get_anomalies_for_date(date, lake_bbox)
            
            for anomaly in anomalies:
                # Find matching cluster (within 50m)
                cluster_id = self._find_matching_cluster(anomaly, anomaly_clusters)
                
                if cluster_id:
                    anomaly_clusters[cluster_id]['years'].append(date.year)
                    anomaly_clusters[cluster_id]['detections'].append(anomaly)
                else:
                    # New cluster
                    cluster_id = f"{anomaly['lat']:.4f}_{anomaly['lon']:.4f}"
                    anomaly_clusters[cluster_id] = {
                        'lat': anomaly['lat'],
                        'lon': anomaly['lon'],
                        'years': [date.year],
                        'detections': [anomaly],
                        'count': 1,
                    }
        
        # Filter for repeatable (appears in N+ years)
        repeatable = []
        for cluster_id, cluster in anomaly_clusters.items():
            unique_years = len(set(cluster['years']))
            
            if unique_years >= REPEATABLE_THRESHOLD:
                repeatable.append({
                    'cluster_id': cluster_id,
                    'lat': cluster['lat'],
                    'lon': cluster['lon'],
                    'years': sorted(list(set(cluster['years']))),
                    'year_count': unique_years,
                    'detection_count': len(cluster['detections']),
                    'avg_score': sum(d.get('score', 0) for d in cluster['detections']) / len(cluster['detections']),
                    'in_database': self._check_if_in_database(cluster),
                })
        
        # Filter out what's already in database
        new_repeatable = [r for r in repeatable if not r['in_database']]
        
        return new_repeatable
    
    def _get_anomalies_for_date(self, date: datetime, bbox: Tuple) -> List[Dict]:
        """
        Get anomalies for specific date and bbox
        
        In production: Query satellite data, run CUDA processing
        For now: Return mock data
        """
        # Mock anomalies for testing
        import random
        random.seed(date.toordinal())  # Reproducible per date
        
        min_lat, min_lon, max_lat, max_lon = bbox
        
        # Generate 5-15 random anomalies
        count = random.randint(5, 15)
        anomalies = []
        
        for _ in range(count):
            anomalies.append({
                'lat': random.uniform(min_lat, max_lat),
                'lon': random.uniform(min_lon, max_lon),
                'score': random.uniform(0.5, 0.95),
                'zscore': random.uniform(2.5, 5.0),
                'date': date.strftime('%Y-%m-%d'),
                'tile_id': f"LANDSAT_{date.strftime('%Y%m%d')}",
            })
        
        return anomalies
    
    def _find_matching_cluster(self, anomaly: Dict, clusters: Dict) -> str:
        """Find existing cluster within 50m tolerance"""
        from detection_sorter import haversine_distance
        
        for cluster_id, cluster in clusters.items():
            distance = haversine_distance(
                anomaly['lat'], anomaly['lon'],
                cluster['lat'], cluster['lon']
            )
            
            if distance < 50:  # 50 meters tolerance
                return cluster_id
        
        return None
    
    def _check_if_in_database(self, cluster: Dict) -> bool:
        """Check if this anomaly is already in database"""
        from detection_sorter import haversine_distance
        
        try:
            with DetectionSorter(self.db_path, admin_mode=True) as sorter:
                sites = sorter.apply()
                
                for site in sites:
                    distance = haversine_distance(
                        cluster['lat'], cluster['lon'],
                        site.lat, site.lon
                    )
                    
                    if distance < 50:  # Within 50m of known site
                        return True
        except:
            pass
        
        return False

# ============================================================================
# DEEP ANALYSIS TRIGGER
# ============================================================================

class DeepAnalysisTrigger:
    """Trigger special tools for new repeatable detections"""
    
    def __init__(self, output_dir: Path):
        self.output_dir = output_dir
    
    def trigger_deep_analysis(self, anomaly: Dict):
        """
        Run special tools on new repeatable anomaly
        
        Special tools:
        - SAR temporal stack
        - SWOT height analysis
        - Thermal multi-date comparison
        - High-res optical review
        """
        
        analysis_file = self.output_dir / f"deep_analysis_{anomaly['cluster_id']}.json"
        
        analysis = {
            'cluster_id': anomaly['cluster_id'],
            'location': {
                'lat': anomaly['lat'],
                'lon': anomaly['lon'],
            },
            'repeatable_years': anomaly['years'],
            'year_count': anomaly['year_count'],
            'detection_count': anomaly['detection_count'],
            'avg_score': anomaly['avg_score'],
            
            # Special tools to run
            'tools_triggered': [
                'sar_temporal_stack',
                'swot_height_analysis',
                'thermal_multi_date',
                'high_res_optical',
            ],
            
            # Status
            'status': 'PENDING',
            'triggered_at': datetime.now().isoformat(),
            'priority': 'HIGH' if anomaly['year_count'] >= 3 else 'MEDIUM',
        }
        
        # Save analysis request
        with open(analysis_file, 'w') as f:
            json.dump(analysis, f, indent=2)
        
        return analysis

# ============================================================================
# FILE OFFLOAD
# ============================================================================

def offload_files(sweep_dates: List[datetime], lake_id: str):
    """
    Offload processed files at end of day
    
    Moves files from data/ to outputs/offload/
    """
    
    offload_subdir = OFFLOAD_DIR / f"{lake_id}_{sweep_dates[0].strftime('%m-%d')}"
    offload_subdir.mkdir(parents=True, exist_ok=True)
    
    # In production: Move actual files
    # For now: Create manifest
    manifest = {
        'lake_id': lake_id,
        'sweep_dates': [d.strftime('%Y-%m-%d') for d in sweep_dates],
        'files_processed': len(sweep_dates) * 10,  # Mock
        'offloaded_at': datetime.now().isoformat(),
        'offload_path': str(offload_subdir),
    }
    
    manifest_file = offload_subdir / "manifest.json"
    with open(manifest_file, 'w') as f:
        json.dump(manifest, f, indent=2)
    
    return manifest

# ============================================================================
# MAIN SMART SCAN
# ============================================================================

def run_smart_scan():
    """Execute smart daily scan with temporal sweep"""
    
    print("="*70)
    print("CESAROPS SMART DAILY SCAN - TEMPORAL SWEEP")
    print("="*70)
    
    # Initialize scheduler
    scheduler = DateSweepScheduler(SCAN_SEASON_START, SCAN_SEASON_END, SWEEP_YEARS)
    
    # Initialize finder
    finder = RepeatableDetectionFinder(DB_PATH)
    
    # Initialize deep analysis trigger
    trigger = DeepAnalysisTrigger(DEEP_ANALYSIS_DIR)
    
    print(f"\nScan Season: {SCAN_SEASON_START[0]:02d}-{SCAN_SEASON_START[1]:02d} to {SCAN_SEASON_END[0]:02d}-{SCAN_SEASON_END[1]:02d}")
    print(f"Years to sweep: {min(SWEEP_YEARS)}-{max(SWEEP_YEARS)}")
    print(f"Total dates: {len(scheduler.dates_to_sweep)}")
    print()
    
    # Process each date
    while scheduler.get_current_date():
        current_date = scheduler.get_current_date()
        month_day = current_date.strftime('%m-%d')
        
        print(f"\n{'='*70}")
        print(f"DATE: {month_day}")
        print(f"{'='*70}")
        
        # Get sweep dates (same month/day, all years)
        sweep_dates = scheduler.get_sweep_dates()
        print(f"Years to process: {[d.year for d in sweep_dates]}")
        
        # Process each lake
        for lake_id, lake_info in sorted(LAKE_BBOXES.items(), key=lambda x: x[1]['scan_order']):
            print(f"\n  Lake: {lake_info['name']}")
            
            # Find repeatable anomalies
            repeatable = finder.find_repeatable_anomalies(
                sweep_dates,
                lake_info['bbox']
            )
            
            print(f"    Repeatable anomalies found: {len(repeatable)}")
            
            # Trigger deep analysis for new ones
            for anomaly in repeatable:
                print(f"    [NEW REPEATABLE] {anomaly['cluster_id']}")
                print(f"       Years: {anomaly['years']}")
                print(f"       Location: {anomaly['lat']:.4f}, {anomaly['lon']:.4f}")
                print(f"       Triggering deep analysis...")
                
                analysis = trigger.trigger_deep_analysis(anomaly)
                print(f"       Priority: {analysis['priority']}")
                print(f"       Tools: {', '.join(analysis['tools_triggered'])}")
            
            # Offload files at end of day
            if repeatable:
                offload = offload_files(sweep_dates, lake_id)
                print(f"    [OFFLOAD] Files offloaded to: {offload['offload_path']}")
        
        # Show progress
        progress = scheduler.get_progress()
        print(f"\n[PROGRESS] {progress['dates_completed']}/{progress['total_dates']} dates ({progress['percent_complete']:.1f}%)")
        
        # Advance to next date
        scheduler.advance()
    
    # Final summary
    print("\n" + "="*70)
    print("SWEEP COMPLETE")
    print("="*70)
    print(f"Total dates processed: {len(scheduler.dates_to_sweep)}")
    print(f"Season: {SCAN_SEASON_START[0]:02d}-{SCAN_SEASON_START[1]:02d} to {SCAN_SEASON_END[0]:02d}-{SCAN_SEASON_END[1]:02d}")
    print(f"Years swept: {min(SWEEP_YEARS)}-{max(SWEEP_YEARS)}")
    print(f"\nDeep analysis requests: {len(list(DEEP_ANALYSIS_DIR.glob('*.json')))}")
    print(f"Offload directories: {len(list(OFFLOAD_DIR.iterdir()))}")
    print("="*70)

# ============================================================================
# MAIN
# ============================================================================

if __name__ == "__main__":
    if len(sys.argv) > 1:
        if sys.argv[1] == '--test':
            # Test mode
            print("Running in TEST mode (single date)")
            SWEEP_YEARS = [2020, 2021, 2022, 2023, 2024, 2025]
    
    run_smart_scan()
