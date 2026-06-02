"""
Semantic clustering of markdown files.
"""
import json
from pathlib import Path
from typing import List, Dict, Optional
import numpy as np
from sklearn.cluster import KMeans
from sklearn.metrics import davies_bouldin_score

class ClusteringEngine:
    """Clusters files into semantic groups."""

    def __init__(self, index_dir: Optional[str] = None):
        """Initialize clustering engine."""
        self.index_dir = Path(index_dir) if index_dir else Path.home() / '.md_index'
        self.index_dir.mkdir(parents=True, exist_ok=True)
        self.clusters_file = self.index_dir / 'clusters.json'
        self.clusters = None

    def cluster(
        self,
        embeddings: np.ndarray,
        file_paths: List[str],
        n_clusters: Optional[int] = None,
        progress_callback=None
    ) -> Dict[int, List[str]]:
        """
        Cluster files into semantic groups.

        Args:
            embeddings: Embedding vectors from EmbeddingEngine
            file_paths: List of file paths corresponding to embeddings
            n_clusters: Number of clusters (auto-estimated if None)
            progress_callback: Optional callback(stage, message)

        Returns:
            Dict mapping cluster_id to list of file_paths
        """
        if len(embeddings) == 0:
            return {}

        # Auto-estimate number of clusters if not provided
        if n_clusters is None:
            n_clusters = max(2, min(50, len(embeddings) // 10))
            if progress_callback:
                progress_callback("clustering", f"Auto-estimated {n_clusters} clusters")

        # Ensure n_clusters doesn't exceed number of files
        n_clusters = min(n_clusters, len(embeddings))

        # Perform K-means clustering
        kmeans = KMeans(n_clusters=n_clusters, random_state=42, n_init=10)
        labels = kmeans.fit_predict(embeddings)

        if progress_callback:
            score = davies_bouldin_score(embeddings, labels)
            progress_callback("clustering", f"Cluster quality score: {score:.3f}")

        # Group files by cluster
        clusters = {}
        for label, file_path in zip(labels, file_paths):
            cluster_id = int(label)
            if cluster_id not in clusters:
                clusters[cluster_id] = []
            clusters[cluster_id].append(file_path)

        self.clusters = clusters
        self.save_clusters()
        return clusters

    def get_cluster_summary(self, cluster_id: int, max_files: int = 5) -> Dict:
        """
        Get summary of a cluster.

        Args:
            cluster_id: Cluster identifier
            max_files: Max files to list in summary

        Returns:
            Summary dict with file count and sample files
        """
        if not self.clusters or cluster_id not in self.clusters:
            return {}

        files = self.clusters[cluster_id]
        return {
            'id': cluster_id,
            'file_count': len(files),
            'sample_files': [Path(f).name for f in files[:max_files]]
        }

    def save_clusters(self) -> None:
        """Save cluster assignments to disk."""
        if not self.clusters:
            return

        data = {
            'cluster_count': len(self.clusters),
            'clusters': {str(k): v for k, v in self.clusters.items()}
        }

        with open(self.clusters_file, 'w') as f:
            json.dump(data, f, indent=2)

    def load_clusters(self) -> bool:
        """Load cluster assignments from disk. Returns True if successful."""
        if not self.clusters_file.exists():
            return False

        try:
            with open(self.clusters_file, 'r') as f:
                data = json.load(f)
            self.clusters = {int(k): v for k, v in data['clusters'].items()}
            return True
        except Exception:
            return False

    def get_cluster_files(self, cluster_id: int) -> List[Dict]:
        """
        Return files in cluster as list of { name, path } dicts.
        """
        if not self.clusters or cluster_id not in self.clusters:
            return []
        files = self.clusters[cluster_id]
        return [{ 'name': Path(f).name, 'path': f } for f in files]

    def list_clusters(self) -> List[Dict]:
        """List all clusters with summaries and file objects."""
        if not self.clusters:
            return []
        return [
            {
                'id': cid,
                'file_count': len(self.clusters[cid]),
                'files': self.get_cluster_files(cid),
                'sample_files': [Path(f).name for f in self.clusters[cid][:5]]
            }
            for cid in sorted(self.clusters.keys())
        ]
