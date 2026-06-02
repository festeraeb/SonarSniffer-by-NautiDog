"""
Search functionality combining semantic and keyword matching.
"""
from pathlib import Path
from typing import List, Tuple, Optional
import re
from md_scanner.embeddings import EmbeddingEngine

class SearchEngine:
    """Combined semantic and keyword search."""

    def __init__(self, embedding_engine: EmbeddingEngine):
        """Initialize with an embedding engine."""
        self.embedding_engine = embedding_engine

    def _extract_keywords(self, query: str) -> List[str]:
        """Extract keywords from query."""
        # Simple tokenization - split on non-alphanumeric
        keywords = re.findall(r'\b\w+\b', query.lower())
        return keywords

    def _keyword_search(
        self,
        keywords: List[str],
        file_paths: List[str],
        top_k: int = 20
    ) -> List[Tuple[str, float]]:
        """
        Keyword-based search in file names and content.

        Args:
            keywords: List of keywords to search for
            file_paths: List of files to search
            top_k: Max results

        Returns:
            List of (file_path, relevance_score) tuples
        """
        if not keywords:
            return []

        scores = {}
        for file_path in file_paths:
            score = 0.0
            file_name = Path(file_path).name.lower()

            # Filename matches get higher weight
            for keyword in keywords:
                if keyword in file_name:
                    score += 2.0

            # Try to match in file content
            try:
                with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                    content = f.read(5000).lower()
                    for keyword in keywords:
                        count = content.count(keyword)
                        if count > 0:
                            score += min(count, 5)
            except Exception:
                pass

            if score > 0:
                scores[file_path] = score

        # Sort by score and return top_k
        sorted_results = sorted(scores.items(), key=lambda x: x[1], reverse=True)
        return [(path, float(score)) for path, score in sorted_results[:top_k]]

    def search(
        self,
        query: str,
        semantic_weight: float = 0.7,
        top_k: int = 15
    ) -> List[Tuple[str, float]]:
        """
        Combined semantic + keyword search.

        Args:
            query: Search query
            semantic_weight: Weight for semantic similarity (0.0-1.0)
            top_k: Number of results to return

        Returns:
            List of (file_path, combined_score) tuples
        """
        if not self.embedding_engine.file_paths:
            return []

        keyword_weight = 1.0 - semantic_weight

        # Get results from both search methods
        semantic_results = self.embedding_engine.search(query, top_k=top_k * 2)
        keywords = self._extract_keywords(query)
        keyword_results = self._keyword_search(
            keywords,
            self.embedding_engine.file_paths,
            top_k=top_k * 2
        )

        # Combine scores
        combined_scores = {}

        # Add semantic results
        for i, (file_path, score) in enumerate(semantic_results):
            # Normalize semantic score
            combined_scores[file_path] = score * semantic_weight

        # Add keyword results
        for file_path, keyword_score in keyword_results:
            # Normalize keyword score (0-2 range -> 0-1 range)
            normalized_keyword_score = min(1.0, keyword_score / 10.0)
            if file_path in combined_scores:
                combined_scores[file_path] += normalized_keyword_score * keyword_weight
            else:
                combined_scores[file_path] = normalized_keyword_score * keyword_weight

        # Sort and return top results
        sorted_results = sorted(
            combined_scores.items(),
            key=lambda x: x[1],
            reverse=True
        )
        return sorted_results[:top_k]

    def search_in_cluster(
        self,
        query: str,
        cluster_files: List[str],
        top_k: int = 10
    ) -> List[Tuple[str, float]]:
        """
        Search within a specific cluster.

        Args:
            query: Search query
            cluster_files: Files in the cluster to search
            top_k: Max results

        Returns:
            List of (file_path, score) tuples
        """
        # Get global search results
        all_results = self.search(query, top_k=len(cluster_files))

        # Filter to only cluster files
        cluster_set = set(cluster_files)
        filtered_results = [
            (path, score) for path, score in all_results
            if path in cluster_set
        ]

        return filtered_results[:top_k]
