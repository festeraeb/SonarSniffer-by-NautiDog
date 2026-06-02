# Offline index sync and caching stubs

import os
import shutil
from pathlib import Path

def cache_index_locally(index_dir: str, cache_dir: str) -> bool:
    """Copy index files to a local cache directory."""
    try:
        os.makedirs(cache_dir, exist_ok=True)
        for fname in ["index.json", "embeddings.json", "clusters.json"]:
            src = Path(index_dir) / fname
            dst = Path(cache_dir) / fname
            if src.exists():
                shutil.copy2(src, dst)
        return True
    except Exception as e:
        print(f"[Offline Sync] Cache error: {e}")
        return False

def export_index(index_dir: str, export_path: str) -> bool:
    """Export index as a zip file for transfer/sharing."""
    import zipfile
    try:
        with zipfile.ZipFile(export_path, 'w') as zf:
            for fname in ["index.json", "embeddings.json", "clusters.json"]:
                fpath = Path(index_dir) / fname
                if fpath.exists():
                    zf.write(fpath, arcname=fname)
        return True
    except Exception as e:
        print(f"[Offline Sync] Export error: {e}")
        return False

def import_index(zip_path: str, target_dir: str) -> bool:
    """Import index from a zip file."""
    import zipfile
    try:
        with zipfile.ZipFile(zip_path, 'r') as zf:
            zf.extractall(target_dir)
        return True
    except Exception as e:
        print(f"[Offline Sync] Import error: {e}")
        return False

