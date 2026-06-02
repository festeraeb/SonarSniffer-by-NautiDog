"""
File scanner for discovering and indexing markdown files.
"""
import os
import json
from pathlib import Path
from datetime import datetime
from typing import List, Dict, Any, Optional
from dataclasses import dataclass, asdict
import re

@dataclass
class FileMetadata:
    """Metadata for a markdown file."""
    path: str
    name: str
    created_time: float
    modified_time: float
    size: int

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

class FileScanner:
    """Scanner for discovering markdown files in a directory tree."""

    # Patterns to exclude (noisy files)
    EXCLUDE_PATTERNS = [
        r'\.venv[\\/]',
        r'venv[\\/]',
        r'env[\\/]',
        r'\.conda[\\/]',
        r'site-packages[\\/]',
        r'\.dist-info[\\/]',
        r'__pycache__[\\/]',
        r'\.pytest_cache[\\/]',
        r'node_modules[\\/]',
        r'^LICENSE\.md$',
        r'^README\.md$',
        r'\.egg-info[\\/]',
    ]

    def __init__(self, index_dir: Optional[str] = None):
        """Initialize scanner with optional index directory."""
        self.index_dir = Path(index_dir) if index_dir else Path.home() / '.md_index'
        self.index_dir.mkdir(parents=True, exist_ok=True)
        self.index_file = self.index_dir / 'files.json'

    def _should_exclude(self, path: str) -> bool:
        """Check if path matches any exclusion patterns."""
        for pattern in self.EXCLUDE_PATTERNS:
            if re.search(pattern, path, re.IGNORECASE):
                return True
        return False

    def scan(self, root_dir: str, progress_callback=None) -> List[FileMetadata]:
        """
        Recursively scan directory for markdown files.

        Args:
            root_dir: Root directory to scan
            progress_callback: Optional callback(current_count) for progress updates

        Returns:
            List of FileMetadata objects
        """
        root_path = Path(root_dir)
        if not root_path.exists():
            raise ValueError(f"Directory does not exist: {root_dir}")

        files = []
        count = 0

        for md_file in root_path.rglob('*.md'):
            rel_path = str(md_file.relative_to(root_path))

            # Skip excluded patterns
            if self._should_exclude(rel_path):
                continue

            try:
                stat_info = md_file.stat()
                metadata = FileMetadata(
                    path=str(md_file.absolute()),
                    name=md_file.name,
                    created_time=stat_info.st_ctime,
                    modified_time=stat_info.st_mtime,
                    size=stat_info.st_size
                )
                files.append(metadata)
                count += 1

                if progress_callback:
                    progress_callback(count)
            except (OSError, PermissionError):
                # Skip files we can't read
                continue

        return sorted(files, key=lambda x: x.modified_time, reverse=True)

    def save_index(self, files: List[FileMetadata]) -> None:
        """Save file index to JSON."""
        data = {
            'indexed_at': datetime.now().isoformat(),
            'file_count': len(files),
            'files': [f.to_dict() for f in files]
        }

        with open(self.index_file, 'w') as f:
            json.dump(data, f, indent=2)

    def load_index(self) -> Optional[List[FileMetadata]]:
        """Load file index from JSON."""
        if not self.index_file.exists():
            return None

        with open(self.index_file, 'r') as f:
            data = json.load(f)

        return [FileMetadata(**f) for f in data['files']]
