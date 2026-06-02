"""
Timeline and recency analysis of markdown files.
"""
from datetime import datetime, timedelta
from typing import List, Dict, Tuple, Optional
import numpy as np
from md_scanner.scanner import FileMetadata

class TimelineEngine:
    """Analyzes file recency and evolution over time."""

    TIME_BUCKETS = [
        ('today', 0),           # today
        ('week', 7),            # last 7 days
        ('month', 30),          # last 30 days
        ('quarter', 90),        # last 90 days
        ('year', 365),          # last year
        ('archived', 999999),   # older than a year
    ]

    def __init__(self):
        """Initialize timeline engine."""
        self.files = None
        self.clusters = None

    def set_data(self, files: List[FileMetadata], clusters: Optional[Dict] = None):
        """Set file metadata and optional cluster data."""
        self.files = files
        self.clusters = clusters

    def get_recency_bucket(self, timestamp: float) -> str:
        """Determine which time bucket a file falls into."""
        now = datetime.now().timestamp()
        age_days = (now - timestamp) / 86400

        for bucket_name, max_days in self.TIME_BUCKETS:
            if age_days <= max_days:
                return bucket_name

        return 'archived'

    def get_timeline_by_date(self, days: int = 30) -> Dict[str, List[str]]:
        """
        Get files organized by date (newest first).

        Args:
            days: Only include files modified in last N days

        Returns:
            Dict mapping date strings to file paths
        """
        if not self.files:
            return {}

        now = datetime.now().timestamp()
        cutoff = now - (days * 86400)

        timeline = {}
        for file_meta in self.files:
            if file_meta.modified_time >= cutoff:
                date_str = datetime.fromtimestamp(
                    file_meta.modified_time
                ).strftime('%Y-%m-%d')

                if date_str not in timeline:
                    timeline[date_str] = []
                timeline[date_str].append(file_meta.path)

        return dict(sorted(timeline.items(), reverse=True))

    def get_bucket_summary(self) -> Dict[str, int]:
        """Get count of files in each time bucket."""
        if not self.files:
            return {}

        buckets = {name: 0 for name, _ in self.TIME_BUCKETS}
        for file_meta in self.files:
            bucket = self.get_recency_bucket(file_meta.modified_time)
            buckets[bucket] += 1

        return buckets

    def find_aged_files(self, days_threshold: int = 180) -> List[Tuple[str, float]]:
        """
        Find files that haven't been modified in a while.

        Args:
            days_threshold: Days of inactivity to flag

        Returns:
            List of (file_path, age_in_days) tuples
        """
        if not self.files:
            return []

        now = datetime.now().timestamp()
        cutoff = now - (days_threshold * 86400)

        old_files = []
        for file_meta in self.files:
            if file_meta.modified_time < cutoff:
                age_days = (now - file_meta.modified_time) / 86400
                old_files.append((file_meta.path, age_days))

        return sorted(old_files, key=lambda x: x[1], reverse=True)

    def get_project_evolution(self, cluster_id: Optional[int] = None) -> Dict[str, int]:
        """
        Track evolution of files in a cluster or all files over time.

        Args:
            cluster_id: Optional cluster to analyze (None = all files)

        Returns:
            Dict mapping month strings to file count
        """
        if not self.files:
            return {}

        files_to_analyze = self.files

        # Filter by cluster if provided
        if cluster_id is not None and self.clusters:
            cluster_files = set(self.clusters.get(cluster_id, []))
            files_to_analyze = [
                f for f in self.files if f.path in cluster_files
            ]

        evolution = {}
        for file_meta in files_to_analyze:
            month_str = datetime.fromtimestamp(
                file_meta.modified_time
            ).strftime('%Y-%m')

            if month_str not in evolution:
                evolution[month_str] = 0
            evolution[month_str] += 1

        return dict(sorted(evolution.items()))
