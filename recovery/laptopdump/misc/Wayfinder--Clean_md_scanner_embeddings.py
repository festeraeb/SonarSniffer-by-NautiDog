"""
Embedding generation and semantic search.
"""
import pickle
from pathlib import Path
from typing import List, Dict, Tuple, Optional
import numpy as np
from sentence_transformers import SentenceTransformer
from sklearn.metrics.pairwise import cosine_similarity

class EmbeddingEngine:
    """Generates and manages embeddings for semantic search."""

    def __init__(self, model_name: str = "all-MiniLM-L6-v2", index_dir: Optional[str] = None):
        """
        Initialize embedding engine.

        Args:
            model_name: HuggingFace model name (default is lightweight and fast)
            index_dir: Directory to cache embeddings
        """
        # Use the best free local model for semantic embeddings
        # all-MiniLM-L6-v2 is free, fast, and works offline
        from sentence_transformers import SentenceTransformer
        self.model = SentenceTransformer(model_name)
        self.index_dir = Path(index_dir) if index_dir else Path.home() / '.md_index'
        self.index_dir.mkdir(parents=True, exist_ok=True)
        self.embeddings_file = self.index_dir / 'embeddings.pkl'
        self.embeddings = None
        self.file_paths = None

    def _extract_content(self, file_path: str, max_chars: int = 2000) -> str:
        """
        Extract content from markdown file for embedding.

        Args:
            file_path: Path to markdown file
            max_chars: Maximum characters to extract

        Returns:
            Extracted text content
        """
        try:
            with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                content = f.read(max_chars)
            return content.strip()
        except Exception:
            return ""

    def generate_embeddings(self, file_paths: List[str], progress_callback=None) -> None:
        """
        Generate embeddings for all files.

        Args:
            file_paths: List of markdown file paths
            progress_callback: Optional callback(current, total) for progress
        """
        self.file_paths = file_paths
        embeddings = []

        for i, file_path in enumerate(file_paths):
            content = self._extract_content(file_path)
            if content:
                embedding = self.model.encode(content, normalize_embeddings=True)
                embeddings.append(embedding)
            else:
                # Use a zero vector for empty files
                embeddings.append(np.zeros(384))

            if progress_callback:
                progress_callback(i + 1, len(file_paths))

        self.embeddings = np.array(embeddings)
        self.save_embeddings()

    def save_embeddings(self) -> None:
        """Save embeddings to disk."""
        data = {
            'embeddings': self.embeddings,
            'file_paths': self.file_paths
        }
        with open(self.embeddings_file, 'wb') as f:
            pickle.dump(data, f)

    def load_embeddings(self) -> bool:
        """Load embeddings from disk. Returns True if successful."""
        if not self.embeddings_file.exists():
            return False

        try:
            with open(self.embeddings_file, 'rb') as f:
                data = pickle.load(f)
            self.embeddings = data['embeddings']
            self.file_paths = data['file_paths']
            return True
        except Exception:
            return False

    def search(self, query: str, top_k: int = 10) -> List[Tuple[str, float]]:
        """
        Find files similar to query.

        Args:
            query: Search query
            top_k: Number of results to return

        Returns:
            List of (file_path, similarity_score) tuples
        """
        if self.embeddings is None or self.file_paths is None:
            raise ValueError("Embeddings not loaded. Call load_embeddings() or generate_embeddings()")

        query_embedding = self.model.encode(query, normalize_embeddings=True)
        similarities = cosine_similarity([query_embedding], self.embeddings)[0]

        # Get top-k results
        top_indices = np.argsort(similarities)[::-1][:top_k]

        results = [
            (self.file_paths[i], float(similarities[i]))
            for i in top_indices
        ]

        return results

    def generate_embeddings_chunked(self, file_paths: List[str], chunk_size: int = 1000, progress_callback=None) -> None:
        """
        Generate embeddings in chunks and save after each chunk.

        Args:
            file_paths: List of markdown file paths
            chunk_size: Number of files per chunk
            progress_callback: Optional callback(current, total, chunk)
        """
        self.file_paths = file_paths
        embeddings = []
        total = len(file_paths)
        for chunk_start in range(0, total, chunk_size):
            chunk_end = min(chunk_start + chunk_size, total)
            chunk_files = file_paths[chunk_start:chunk_end]
            chunk_embeddings = []
            for i, file_path in enumerate(chunk_files):
                content = self._extract_content(file_path)
                if content:
                    embedding = self.model.encode(content, normalize_embeddings=True)
                    chunk_embeddings.append(embedding)
                else:
                    chunk_embeddings.append(np.zeros(384))
                if progress_callback:
                    progress_callback(chunk_start + i + 1, total, chunk_start // chunk_size + 1)
            embeddings.extend(chunk_embeddings)
            self.embeddings = np.array(embeddings)
            self.save_embeddings()
        self.embeddings = np.array(embeddings)
        self.save_embeddings()

def prewarm_torch(model_name: str = "all-MiniLM-L6-v2"):
    """Pre-warm torch and the embedding model to reduce first-use latency."""
    try:
        import torch
        from sentence_transformers import SentenceTransformer
        _ = SentenceTransformer(model_name)
        if torch.cuda.is_available():
            torch.zeros(1).cuda()
        print("[Prewarm] Torch and model loaded.")
    except Exception as e:
        print(f"[Prewarm] Error: {e}")
