#!/usr/bin/env python3
"""
Python bridge for Tauri subprocess communication.
Receives JSON commands from Rust and calls Python engines accordingly.
"""

import json
import sys
from pathlib import Path
from typing import Any, Dict, Optional, Callable

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))

from md_scanner.scanner import FileScanner
from md_scanner.embeddings import EmbeddingEngine
from md_scanner.clustering import ClusteringEngine
from md_scanner.search import SearchEngine
from md_scanner.timeline import TimelineEngine
from md_scanner.learning import AdaptiveCoach


class TauriBridge:
    """Bridge between Tauri commands and Python engines."""

    def __init__(self):
        self.scan_engine = None
        self.embedding_engine = None
        self.cluster_engine = None
        self.search_engine = None
        self.coach = AdaptiveCoach()

    def handle_command(self, method: str, args: Dict[str, Any]) -> Dict[str, Any]:
        """Route command to appropriate handler."""
        try:
            handler = getattr(self, f"_{method}", None)
            if handler is None:
                return {"error": f"Unknown method: {method}"}
            
            result = handler(**args)
            return {"success": True, "data": result}
        except Exception as e:
            return {"error": str(e), "success": False}

    def _scan_directory(
        self,
        path: str,
        index_dir: str,
        progress_callback: Optional[Callable] = None,
    ) -> Dict[str, Any]:
        """Scan a directory and create index."""
        self.scan_engine = FileScanner(index_dir)
        result = self.scan_engine.scan(path)
        
        return {
            "files_scanned": len(result.get("files", [])),
            "total_size": result.get("total_size", 0),
            "index_path": index_dir,
        }

    def _generate_embeddings(
        self,
        index_dir: str,
        progress_callback: Optional[Callable] = None,
    ) -> Dict[str, Any]:
        """Generate embeddings for indexed files."""
        self.embedding_engine = EmbeddingEngine(index_dir)
        cached, generated = self.embedding_engine.generate()
        
        return {
            "embeddings_generated": generated,
            "cached_count": cached,
        }

    def _create_clusters(
        self,
        index_dir: str,
        num_clusters: Optional[int] = None,
        progress_callback: Optional[Callable] = None,
    ) -> Dict[str, Any]:
        """Create clusters from embeddings."""
        self.cluster_engine = ClusteringEngine(index_dir)
        
        if num_clusters is None:
            # Auto-detect optimal cluster count
            num_clusters = self.cluster_engine.estimate_clusters()
        
        result = self.cluster_engine.cluster(num_clusters)
        
        return {
            "clusters_created": len(result.get("clusters", [])),
            "total_files": result.get("total_files", 0),
        }

    def _search(
        self,
        query: str,
        index_dir: str,
        top_k: int = 10,
        semantic_weight: float = 0.7,
    ) -> Dict[str, Any]:
        """Search index with query."""
        self.search_engine = SearchEngine(index_dir)
        results = self.search_engine.search(
            query, top_k=top_k, semantic_weight=semantic_weight
        )
        
        return results

    def _get_clusters_summary(self, index_dir: str) -> Dict[str, Any]:
        """Get clusters summary."""
        self.cluster_engine = ClusteringEngine(index_dir)
        return self.cluster_engine.get_summary()

    def _get_timeline(self, index_dir: str, days: int = 30) -> Dict[str, Any]:
        """Get timeline data."""
        timeline = TimelineEngine(index_dir)
        return timeline.get_timeline(days)

    def _get_stats(self, index_dir: str) -> Dict[str, Any]:
        """Get index statistics."""
        index_path = Path(index_dir)
        files_json = index_path / "files.json"
        embeddings_pkl = index_path / "embeddings.pkl"
        clusters_json = index_path / "clusters.json"
        
        total_files = 0
        total_size = 0
        embeddings_count = 0
        cluster_count = 0
        
        if files_json.exists():
            import json as json_module
            with open(files_json) as f:
                data = json_module.load(f)
                total_files = len(data.get("files", []))
                total_size = sum(f.get("size", 0) for f in data.get("files", []))
        
        if embeddings_pkl.exists():
            import pickle
            with open(embeddings_pkl, "rb") as f:
                data = pickle.load(f)
                embeddings_count = len(data.get("embeddings", {}))
        
        if clusters_json.exists():
            import json as json_module
            with open(clusters_json) as f:
                data = json_module.load(f)
                cluster_count = len(data.get("clusters", []))
        
        return {
            "total_files": total_files,
            "total_size_bytes": total_size,
            "cluster_count": cluster_count,
            "embeddings_count": embeddings_count,
            "last_updated": "",  # TODO: Get from file metadata
            "age_buckets": [],  # TODO: Calculate age distribution
        }

    def _validate_index(self, index_dir: str) -> Dict[str, Any]:
        """Validate index state."""
        index_path = Path(index_dir)
        
        has_files = (index_path / "files.json").exists()
        has_embeddings = (index_path / "embeddings.pkl").exists()
        has_clusters = (index_path / "clusters.json").exists()
        
        index_valid = has_files  # At minimum, need scanned files
        
        if not has_files:
            message = "No index found. Start by scanning a directory."
        elif not has_embeddings:
            message = "Index found. Generate embeddings to continue."
        elif not has_clusters:
            message = "Embeddings ready. Create clusters to organize."
        else:
            message = "Index complete. Ready to search."
        
        return {
            "has_files": has_files,
            "has_embeddings": has_embeddings,
            "has_clusters": has_clusters,
            "index_valid": index_valid,
            "message": message,
        }

    def _get_system_info(self) -> Dict[str, Any]:
        """Get system information."""
        import os
        import platform
        
        return {
            "os": platform.system(),
            "arch": platform.machine(),
            "cpu_cores": os.cpu_count() or 1,
            "available_memory_gb": 0.0,  # TODO: Implement memory detection
            "device_type": "desktop",  # TODO: Detect if tablet
        }

    # =========================================================================
    # Adaptive Coaching Commands
    # =========================================================================

    def _coach_start_session(self) -> Dict[str, Any]:
        """Start a coaching session."""
        session_id = self.coach.start_session()
        return {"session_id": session_id}

    def _coach_end_session(self) -> Dict[str, Any]:
        """End the current coaching session."""
        return self.coach.end_session()

    def _coach_get_naming_suggestion(
        self,
        filename: str,
        content: Optional[str] = None,
        file_path: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Get naming suggestion with fade-out logic."""
        from dataclasses import asdict
        result = self.coach.get_naming_suggestion(filename, content, file_path)
        return asdict(result)

    def _coach_get_folder_suggestion(
        self,
        file_path: str,
        available_folders: Optional[list] = None,
    ) -> Dict[str, Any]:
        """Get folder suggestion with fade-out logic."""
        from dataclasses import asdict
        result = self.coach.get_folder_suggestion(file_path, available_folders)
        return asdict(result)

    def _coach_get_search_tip(
        self,
        query: str,
        results_count: int,
    ) -> Dict[str, Any]:
        """Get search tip if user is struggling."""
        tip = self.coach.get_search_tip(query, results_count)
        return {"tip": tip}

    def _coach_record_response(
        self,
        suggestion_type: str,
        accepted: bool,
        original_value: str,
        suggested_value: str,
        final_value: str,
    ) -> Dict[str, Any]:
        """Record user response to a suggestion."""
        self.coach.record_suggestion_response(
            suggestion_type, accepted, original_value, suggested_value, final_value
        )
        return {"recorded": True}

    def _coach_get_status(self) -> Dict[str, Any]:
        """Get current coaching status."""
        return self.coach.get_status()

    def _coach_get_fade_status(self) -> Dict[str, Any]:
        """Get fade-out status for all skills."""
        return self.coach.get_fade_status()

    def _coach_set_override(
        self,
        skill_name: str,
        disabled: bool,
    ) -> Dict[str, Any]:
        """Set user override for skill suggestions."""
        self.coach.set_fade_out_override(skill_name, disabled)
        return {"set": True, "skill": skill_name, "disabled": disabled}

    def _coach_reset_tracking(
        self,
        skill_name: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Reset skill tracking."""
        self.coach.reset_skill_tracking(skill_name)
        return {"reset": True, "skill": skill_name or "all"}

    def _coach_force_reengagement(
        self,
        skill_name: str,
    ) -> Dict[str, Any]:
        """Force re-engagement of suggestions for a skill."""
        self.coach.force_re_engagement(skill_name)
        return {"reengaged": True, "skill": skill_name}


def main():
    """Main entry point for subprocess communication."""
    bridge = TauriBridge()
    
    try:
        # Read JSON command from stdin
        input_data = sys.stdin.read()
        command = json.loads(input_data)
        
        method = command.get("method")
        args = command.get("args", {})
        
        result = bridge.handle_command(method, args)
        
        # Write result to stdout
        print(json.dumps(result))
    except Exception as e:
        error_response = {
            "error": str(e),
            "success": False,
        }
        print(json.dumps(error_response), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
