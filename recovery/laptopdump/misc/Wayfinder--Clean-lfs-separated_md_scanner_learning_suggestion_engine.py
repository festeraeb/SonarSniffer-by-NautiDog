"""
Suggestion Engine - Generates naming and organization suggestions

Suggests:
- File names based on content analysis
- Folder structures based on file types and topics
- Tags for metadata
- Conventions to adopt

Learns from:
- User's existing naming patterns
- User's folder structure
- User's approval/rejection of suggestions
- Industry best practices
"""

from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Tuple
from dataclasses import dataclass, asdict
import re
import os
from sentence_transformers import SentenceTransformer

from .behavior_tracker import BehaviorTracker


@dataclass
class NamingSuggestion:
    """A suggested name for a file"""

    suggested_name: str
    original_name: str
    confidence: float  # How confident we are this is a good name
    reasoning: str  # Why we suggested this
    alternatives: List[str]  # Other options
    convention_used: str  # Which naming convention we applied
    category: Optional[str]  # Detected category


@dataclass
class FolderSuggestion:
    """A suggested folder location for a file"""

    suggested_path: str
    original_path: str
    reasoning: str
    confidence: float
    creates_new_folder: bool
    moves_to_existing: bool


@dataclass
class ConventionSuggestion:
    """A suggested naming/organization convention"""

    convention_name: str
    description: str
    examples: List[str]
    applies_to: List[str]  # File types this applies to
    priority: int  # Higher = more important


class SuggestionEngine:
    """
    Generates intelligent naming and organization suggestions
    based on content analysis and user behavior.
    """

    # Built-in naming conventions
    NAMING_CONVENTIONS = {
        "date_prefix": {
            "pattern": "{YYYYMMDD}_{name}",
            "description": "Date first, then descriptive name",
            "examples": ["20260209_meeting_notes", "20260115_budget_review"],
            "best_for": ["notes", "reports", "meetings", "logs"],
        },
        "category_first": {
            "pattern": "{CATEGORY}_{topic}_{detail}",
            "description": "Category prefix for easy sorting",
            "examples": ["DOCS_project_requirements", "CODE_utils_helpers"],
            "best_for": ["code", "documents", "mixed"],
        },
        "project_based": {
            "pattern": "{project}_{type}_{name}",
            "description": "Project name first for project-centric work",
            "examples": ["wayfinder_design_mockups", "clientx_invoice_march"],
            "best_for": ["project files", "client work", "deliverables"],
        },
        "semantic": {
            "pattern": "{what}_{context}_{version}",
            "description": "Descriptive naming based on content",
            "examples": ["budget_2026_v2", "api_documentation_final"],
            "best_for": ["versioned documents", "drafts", "evolving files"],
        },
    }

    # Folder structure templates
    FOLDER_STRUCTURES = {
        "by_type": {
            "structure": {
                "Documents": ["*.docx", "*.pdf", "*.txt"],
                "Spreadsheets": ["*.xlsx", "*.csv"],
                "Code": ["*.py", "*.js", "*.ts"],
                "Images": ["*.jpg", "*.png", "*.gif"],
                "Archives": ["Old", "Backup"],
            },
            "description": "Organize by file type",
        },
        "by_project": {
            "structure": {
                "{Project}": {
                    "docs": ["requirements", "design", "notes"],
                    "src": ["code files"],
                    "assets": ["images", "resources"],
                    "output": ["deliverables", "exports"],
                }
            },
            "description": "Organize around projects",
        },
        "by_date": {
            "structure": {"{YYYY}": {"{MM}": ["files from that month"]}},
            "description": "Organize by date hierarchy",
        },
        "gtd": {
            "structure": {
                "Inbox": ["unprocessed files"],
                "Active": ["current work"],
                "Reference": ["lookup materials"],
                "Archive": ["completed work"],
            },
            "description": "Getting Things Done methodology",
        },
    }

    def __init__(self, tracker: BehaviorTracker, data_dir: str = None):
        self.tracker = tracker
        self.data_dir = (
            Path(data_dir) if data_dir else Path("~/.wayfinder").expanduser()
        )

        # Load user's learned conventions
        self.user_conventions = self._learn_user_conventions()

        self.embedder = None
        try:
            self.embedder = SentenceTransformer('all-MiniLM-L6-v2')
        except Exception as e:
            self.embedder = None
            print(f"Embedding model not available: {e}")

    def _learn_user_conventions(self) -> Dict:
        """Learn naming conventions from user's actual files"""
        naming_prefs = self.tracker.get_naming_preferences()
        patterns = naming_prefs.get("learned_patterns", {})

        if not patterns:
            return {}

        learned = {
            "separator": (
                "_"
                if patterns.get("uses_underscores", 0) > patterns.get("uses_hyphens", 0)
                else "-"
            ),
            "uses_dates": patterns.get("date_frequency", 0) > 0.3,
            "case_style": (
                "lowercase"
                if patterns.get("uses_lowercase", 0) > patterns.get("uses_camelCase", 0)
                else "camelCase"
            ),
            "common_prefixes": [p["prefix"] for p in patterns.get("uses_prefixes", [])],
            "avg_length": patterns.get("average_length", 25),
        }

        return learned

    def get_content_embedding(self, text: str) -> Optional[List[float]]:
        """Generate embedding for given text using sentence-transformers."""
        if self.embedder:
            return self.embedder.encode(text).tolist()
        return None

    def suggest_filename(
        self,
        file_path: str,
        content_analysis: Optional[Dict] = None,
        file_content: Optional[str] = None,
    ) -> NamingSuggestion:
        """
        Suggest a filename based on content analysis and embeddings.

        Args:
            file_path: Current file path
            content_analysis: Dict with 'title', 'topics', 'type', etc.
            file_content: The actual content of the file for embedding

        Returns:
            NamingSuggestion with primary suggestion and alternatives
        """
        original_name = Path(file_path).stem
        extension = Path(file_path).suffix

        # Extract content info
        if content_analysis:
            title = content_analysis.get("title", "")
            topics = content_analysis.get("topics", [])
            file_type = content_analysis.get("type", "document")
            date_hint = content_analysis.get("date")
        else:
            title = original_name
            topics = []
            file_type = self._infer_type_from_extension(extension)
            date_hint = None

        # Determine best convention
        convention = self._choose_convention(file_type, topics)

        # Generate name based on convention
        suggested = self._apply_convention(
            convention=convention,
            title=title,
            topics=topics,
            file_type=file_type,
            date=date_hint or datetime.now().strftime("%Y%m%d"),
        )

        # Generate alternatives with other conventions
        alternatives = []
        for alt_convention in self.NAMING_CONVENTIONS:
            if alt_convention != convention:
                alt_name = self._apply_convention(
                    convention=alt_convention,
                    title=title,
                    topics=topics,
                    file_type=file_type,
                    date=date_hint or datetime.now().strftime("%Y%m%d"),
                )
                if alt_name != suggested:
                    alternatives.append(alt_name + extension)

        # Detect category
        category = self._detect_category(file_type, topics, extension)

        # Calculate confidence
        confidence = self._calculate_naming_confidence(
            original_name, suggested, content_analysis
        )

        embedding = None
        if file_content:
            embedding = self.get_content_embedding(file_content)
        # Use embedding in content_analysis if available
        if content_analysis is None:
            content_analysis = {}
        if embedding:
            content_analysis['embedding'] = embedding

        return NamingSuggestion(
            suggested_name=suggested + extension,
            original_name=original_name + extension,
            confidence=confidence,
            reasoning=self._generate_naming_reasoning(convention, title, category),
            alternatives=alternatives[:3],
            convention_used=convention,
            category=category,
        )

    def suggest_folder(
        self,
        file_path: str,
        base_directory: str,
        content_analysis: Optional[Dict] = None,
        file_content: Optional[str] = None,
    ) -> FolderSuggestion:
        """
        Suggest a folder location for a file, using embeddings if available.

        Args:
            file_path: Current file path
            base_directory: Root directory to organize into
            content_analysis: Content info dict
            file_content: The actual content of the file for embedding

        Returns:
            FolderSuggestion with recommended location
        """
        original_path = str(Path(file_path).parent)
        extension = Path(file_path).suffix

        # Get file type
        file_type = (
            content_analysis.get("type", "document") if content_analysis else "document"
        )
        category = self._detect_category(file_type, [], extension)

        embedding = None
        if file_content:
            embedding = self.get_content_embedding(file_content)
        if content_analysis is None:
            content_analysis = {}
        if embedding:
            content_analysis['embedding'] = embedding

        # Check user's existing folder structure
        existing_folders = self._analyze_existing_folders(base_directory)

        # Find best matching folder
        suggested_path, reasoning, creates_new = self._find_best_folder(
            base_directory=base_directory,
            category=category,
            file_type=file_type,
            extension=extension,
            existing_folders=existing_folders,
            content_analysis=content_analysis,
        )

        confidence = 0.8 if not creates_new else 0.6

        return FolderSuggestion(
            suggested_path=suggested_path,
            original_path=original_path,
            reasoning=reasoning,
            confidence=confidence,
            creates_new_folder=creates_new,
            moves_to_existing=not creates_new,
        )

    def suggest_convention(
        self, file_types: List[str], current_patterns: Optional[Dict] = None
    ) -> ConventionSuggestion:
        """
        Suggest a naming convention for the user to adopt.

        Based on their file types and current (possibly inconsistent) patterns.
        """
        # Determine dominant file types
        code_heavy = any(t in ["py", "js", "ts", "java", "cpp"] for t in file_types)
        doc_heavy = any(t in ["docx", "pdf", "txt", "md"] for t in file_types)
        data_heavy = any(t in ["xlsx", "csv", "json"] for t in file_types)

        if code_heavy:
            # Developers prefer semantic naming
            return ConventionSuggestion(
                convention_name="semantic",
                description=(
                    "Descriptive names that explain what the file contains. "
                    "Use lowercase with underscores. Include version if needed."
                ),
                examples=[
                    "api_client_v2.py",
                    "user_authentication_tests.py",
                    "database_schema_migrations.sql",
                ],
                applies_to=["*.py", "*.js", "*.ts", "*.java"],
                priority=1,
            )

        elif doc_heavy:
            # Document workers benefit from date + category
            return ConventionSuggestion(
                convention_name="date_prefix",
                description=(
                    "Start with date (YYYYMMDD) for easy chronological sorting. "
                    "Follow with category and description."
                ),
                examples=[
                    "20260209_meeting_standup_notes.docx",
                    "20260115_report_quarterly_sales.pdf",
                    "20261231_plan_2027_roadmap.md",
                ],
                applies_to=["*.docx", "*.pdf", "*.txt", "*.md"],
                priority=1,
            )

        elif data_heavy:
            # Data files need source + date + description
            return ConventionSuggestion(
                convention_name="category_first",
                description=(
                    "Start with data source or category. Include date if time-sensitive. "
                    "End with description."
                ),
                examples=[
                    "SALES_2026Q1_northeast_region.xlsx",
                    "CUSTOMERS_export_20260209.csv",
                    "INVENTORY_warehouse_a_current.xlsx",
                ],
                applies_to=["*.xlsx", "*.csv", "*.json"],
                priority=1,
            )

        else:
            # General purpose: category first is versatile
            return ConventionSuggestion(
                convention_name="category_first",
                description=(
                    "Start with a category prefix (DOCS, CODE, DATA, etc.) "
                    "for easy grouping when sorted alphabetically."
                ),
                examples=[
                    "DOCS_project_proposal.pdf",
                    "IMG_product_screenshot.png",
                    "NOTES_brainstorm_session.md",
                ],
                applies_to=["*"],
                priority=1,
            )

    def _apply_convention(
        self, convention: str, title: str, topics: List[str], file_type: str, date: str
    ) -> str:
        """Apply a naming convention to generate a filename"""

        # Clean the title
        clean_title = self._clean_for_filename(title)

        # Get separator from user preferences or default
        sep = self.user_conventions.get("separator", "_")

        if convention == "date_prefix":
            # YYYYMMDD_description
            return f"{date}{sep}{clean_title}"

        elif convention == "category_first":
            # CATEGORY_topic_description
            category = self._infer_category_prefix(file_type, topics)
            topic = topics[0] if topics else "general"
            topic = self._clean_for_filename(topic)
            return f"{category}{sep}{topic}{sep}{clean_title}"

        elif convention == "project_based":
            # project_type_description
            project = topics[0] if topics else "misc"
            project = self._clean_for_filename(project)
            return f"{project}{sep}{file_type}{sep}{clean_title}"

        elif convention == "semantic":
            # what_context_version
            return clean_title

        else:
            return clean_title

    def _clean_for_filename(self, text: str) -> str:
        """Clean text for use as filename"""
        if not text:
            return "untitled"

        # Remove invalid characters
        text = re.sub(r'[<>:"/\\|?*]', "", text)

        # Replace spaces and special chars with separator
        sep = self.user_conventions.get("separator", "_")
        text = re.sub(r"[\s\-\.]+", sep, text)

        # Apply case convention
        case_style = self.user_conventions.get("case_style", "lowercase")
        if case_style == "lowercase":
            text = text.lower()

        # Trim to reasonable length
        max_len = min(self.user_conventions.get("avg_length", 50), 100)
        if len(text) > max_len:
            text = text[:max_len].rsplit(sep, 1)[0]  # Cut at last separator

        return text.strip(sep)

    def _choose_convention(self, file_type: str, topics: List[str]) -> str:
        """Choose best naming convention for this file"""

        # Check if user has strong preferences from their patterns
        if self.user_conventions.get("uses_dates"):
            return "date_prefix"

        if self.user_conventions.get("common_prefixes"):
            return "category_first"

        # Otherwise, choose based on file type
        type_conventions = {
            "document": "date_prefix",
            "code": "semantic",
            "spreadsheet": "category_first",
            "image": "semantic",
            "presentation": "project_based",
        }

        return type_conventions.get(file_type, "category_first")

    def _infer_type_from_extension(self, extension: str) -> str:
        """Infer file type from extension"""
        ext = extension.lower().lstrip(".")

        type_map = {
            "docx": "document",
            "doc": "document",
            "pdf": "document",
            "txt": "document",
            "md": "document",
            "rtf": "document",
            "odt": "document",
            "xlsx": "spreadsheet",
            "xls": "spreadsheet",
            "csv": "spreadsheet",
            "ods": "spreadsheet",
            "pptx": "presentation",
            "ppt": "presentation",
            "odp": "presentation",
            "py": "code",
            "js": "code",
            "ts": "code",
            "java": "code",
            "cpp": "code",
            "c": "code",
            "go": "code",
            "rs": "code",
            "jpg": "image",
            "jpeg": "image",
            "png": "image",
            "gif": "image",
            "svg": "image",
            "webp": "image",
        }

        return type_map.get(ext, "file")

    def _detect_category(
        self, file_type: str, topics: List[str], extension: str
    ) -> str:
        """Detect category for the file"""
        # Check topics for hints
        topic_categories = {
            "meeting": "meetings",
            "notes": "notes",
            "report": "reports",
            "budget": "finance",
            "invoice": "finance",
            "design": "design",
            "test": "tests",
            "readme": "documentation",
            "config": "config",
        }

        for topic in topics:
            topic_lower = topic.lower()
            for keyword, category in topic_categories.items():
                if keyword in topic_lower:
                    return category

        # Fall back to file type
        type_categories = {
            "document": "documents",
            "spreadsheet": "data",
            "presentation": "presentations",
            "code": "code",
            "image": "images",
        }

        return type_categories.get(file_type, "misc")

    def _infer_category_prefix(self, file_type: str, topics: List[str]) -> str:
        """Generate a category prefix like DOCS, CODE, etc."""
        category = self._detect_category(file_type, topics, "")

        prefix_map = {
            "documents": "DOCS",
            "data": "DATA",
            "presentations": "PRES",
            "code": "CODE",
            "images": "IMG",
            "meetings": "MTG",
            "notes": "NOTES",
            "reports": "RPT",
            "finance": "FIN",
            "design": "DSGN",
            "tests": "TEST",
            "documentation": "DOCS",
            "config": "CFG",
        }

        return prefix_map.get(category, "MISC")

    def _analyze_existing_folders(self, base_directory: str) -> Dict[str, List[str]]:
        """Analyze existing folder structure"""
        base = Path(base_directory)
        folder_contents = {}

        if not base.exists():
            return folder_contents

        for folder in base.iterdir():
            if folder.is_dir() and not folder.name.startswith("."):
                # Get file extensions in this folder
                extensions = set()
                try:
                    for f in folder.iterdir():
                        if f.is_file():
                            extensions.add(f.suffix.lower())
                except PermissionError:
                    pass

                folder_contents[folder.name] = list(extensions)

        return folder_contents

    def _find_best_folder(
        self,
        base_directory: str,
        category: str,
        file_type: str,
        extension: str,
        existing_folders: Dict[str, List[str]],
        content_analysis: Optional[Dict],
    ) -> Tuple[str, str, bool]:
        """Find or suggest the best folder for a file"""
        base = Path(base_directory)

        # First, try to match by extension
        for folder_name, extensions in existing_folders.items():
            if extension.lower() in extensions:
                return (
                    str(base / folder_name),
                    f"Matches existing folder for {extension} files",
                    False,
                )

        # Second, try to match by category name
        category_matches = [
            name
            for name in existing_folders
            if category.lower() in name.lower() or name.lower() in category.lower()
        ]
        if category_matches:
            return (
                str(base / category_matches[0]),
                f"Matches category '{category}'",
                False,
            )

        # Third, suggest creating a new folder
        new_folder = category.title()
        return (str(base / new_folder), f"New folder for {category} files", True)

    def _calculate_naming_confidence(
        self, original: str, suggested: str, content_analysis: Optional[Dict]
    ) -> float:
        """Calculate confidence in the naming suggestion"""
        confidence = 0.5  # Base confidence

        # Higher confidence if we have content analysis
        if content_analysis:
            if content_analysis.get("title"):
                confidence += 0.2
            if content_analysis.get("topics"):
                confidence += 0.1

        # Higher confidence if name is significantly different
        if original.lower() != suggested.lower():
            confidence += 0.1

        # Lower confidence if suggested name is very short
        if len(suggested) < 5:
            confidence -= 0.2

        return min(max(confidence, 0.1), 1.0)

    def _generate_naming_reasoning(
        self, convention: str, title: str, category: str
    ) -> str:
        """Generate human-readable reasoning"""
        convention_info = self.NAMING_CONVENTIONS.get(convention, {})

        if convention == "date_prefix":
            return f"Date prefix for chronological sorting. Category: {category}"
        elif convention == "category_first":
            return f"Category prefix ({category.upper()}) for easy grouping"
        elif convention == "project_based":
            return f"Project-based naming for related files"
        elif convention == "semantic":
            return f"Descriptive name based on content: {title[:30]}..."
        else:
            return f"Based on detected content and category: {category}"

    # === Batch Suggestions ===

    def suggest_batch_rename(
        self, files: List[str], common_convention: Optional[str] = None
    ) -> List[NamingSuggestion]:
        """Generate suggestions for multiple files at once"""
        suggestions = []

        # If no convention specified, analyze files to determine best one
        if not common_convention:
            extensions = [Path(f).suffix for f in files]
            file_type = (
                self._infer_type_from_extension(extensions[0]) if extensions else "file"
            )
            common_convention = self._choose_convention(file_type, [])

        for file_path in files:
            suggestion = self.suggest_filename(
                file_path=file_path, force_convention=common_convention
            )
            suggestions.append(suggestion)

        return suggestions

    def suggest_folder_structure(
        self, files: List[str], base_directory: str
    ) -> Dict[str, List[str]]:
        """Suggest a complete folder structure for a set of files"""
        structure = {}

        for file_path in files:
            extension = Path(file_path).suffix.lower()
            file_type = self._infer_type_from_extension(extension)
            category = self._detect_category(file_type, [], extension)

            folder = category.title()
            if folder not in structure:
                structure[folder] = []
            structure[folder].append(file_path)

        return structure
