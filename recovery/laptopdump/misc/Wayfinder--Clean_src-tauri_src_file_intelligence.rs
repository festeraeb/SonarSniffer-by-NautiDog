// File Intelligence Module
// Provides smart file organization suggestions and pattern learning

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use chrono::{DateTime, Local};

// ============================================================================
// TYPES & STRUCTURES
// ============================================================================

/// Supported document types for organization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DocumentType {
    // Office documents
    Word,           // .docx, .doc
    Excel,          // .xlsx, .xls
    PowerPoint,     // .pptx, .ppt
    PDF,            // .pdf
    
    // OpenOffice
    OpenDocument,   // .odt, .ods, .odp
    
    // Text/Notes
    Markdown,       // .md
    PlainText,      // .txt
    RichText,       // .rtf
    
    // Code (handled separately by Git Clippy)
    Code,           // .py, .js, .ts, .rs, etc.
    
    // Other
    Image,          // .jpg, .png, .gif
    Archive,        // .zip, .rar, .7z
    Unknown,
}

impl DocumentType {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "docx" | "doc" => DocumentType::Word,
            "xlsx" | "xls" | "csv" => DocumentType::Excel,
            "pptx" | "ppt" => DocumentType::PowerPoint,
            "pdf" => DocumentType::PDF,
            "odt" | "ods" | "odp" => DocumentType::OpenDocument,
            "md" | "markdown" => DocumentType::Markdown,
            "txt" => DocumentType::PlainText,
            "rtf" => DocumentType::RichText,
            "py" | "js" | "ts" | "tsx" | "jsx" | "rs" | "go" | "java" | "c" | "cpp" | "h" | "cs" => DocumentType::Code,
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "svg" => DocumentType::Image,
            "zip" | "rar" | "7z" | "tar" | "gz" => DocumentType::Archive,
            _ => DocumentType::Unknown,
        }
    }
    
    pub fn is_organizable(&self) -> bool {
        matches!(self, 
            DocumentType::Word | 
            DocumentType::Excel | 
            DocumentType::PowerPoint | 
            DocumentType::PDF |
            DocumentType::OpenDocument |
            DocumentType::Markdown |
            DocumentType::PlainText |
            DocumentType::RichText
        )
    }
}

/// A discovered document with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDocument {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub doc_type: DocumentType,
    pub size_bytes: u64,
    pub modified: String,
    pub parent_dir: String,
    pub depth: usize,
    pub siblings_count: usize,      // Files in same directory
    pub similar_siblings: usize,    // Files of same type in directory
}

/// Suggestion for organizing a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationSuggestion {
    pub file_path: String,
    pub file_name: String,
    pub action: SuggestionAction,
    pub confidence: f32,            // 0.0 to 1.0
    pub reason: String,
    pub category: String,           // e.g., "Resumes", "School Notes", "Receipts"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestionAction {
    Move { to_path: String },
    Rename { new_name: String },
    CreateSubfolder { folder_name: String },
    Archive,
    LeaveAlone,
}

/// Naming pattern detected from user's files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamingPattern {
    pub pattern_type: PatternType,
    pub example: String,
    pub frequency: usize,           // How many files match this pattern
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PatternType {
    DatePrefix,         // 2026-02-10-filename.docx
    DateSuffix,         // filename-2026-02-10.docx
    CategoryFirst,      // notes-meeting-budget.md
    ProjectBased,       // wayfinder-setup-guide.md
    VersionNumbered,    // report-v3.docx
    Semantic,           // quarterly-budget-analysis.docx
    Unstructured,       // Document1.docx
}

/// User's learned preferences
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserPreferences {
    pub preferred_naming: Option<PatternType>,
    pub preferred_structure: FolderStructure,
    pub dismissed_suggestions: Vec<String>,  // File paths that user said "leave alone"
    pub custom_categories: HashMap<String, String>,  // "Resumes" -> "~/Documents/Career/Resumes"
    pub suggestion_frequency: SuggestionFrequency,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum FolderStructure {
    #[default]
    ByType,         // Documents/Word, Documents/PDF, etc.
    ByProject,      // Documents/ProjectA, Documents/ProjectB
    ByDate,         // Documents/2026/02
    ByCategory,     // Documents/Work, Documents/Personal
    GTD,            // Inbox, Active, Archive, Reference
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SuggestionFrequency {
    Always,         // Prompt on every save
    #[default]
    Smart,          // Only when confidence is high
    Daily,          // Batch suggestions once per day
    Weekly,         // Weekly digest only
    Never,          // Disabled
}

/// Statistics about a scan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanStatistics {
    pub total_documents: usize,
    pub by_type: HashMap<String, usize>,
    pub by_location: HashMap<String, usize>,
    pub potential_duplicates: usize,
    pub unorganized_count: usize,
    pub naming_score: f32,          // 0-100 how consistent is naming
}

// ============================================================================
// CORE FUNCTIONS
// ============================================================================

/// Scan a directory for organizable documents
pub fn scan_for_documents(root_path: &str, max_depth: Option<usize>) -> Result<Vec<DiscoveredDocument>, String> {
    let root = Path::new(root_path);
    if !root.exists() {
        return Err(format!("Path does not exist: {}", root_path));
    }
    
    let mut documents = Vec::new();
    let max_d = max_depth.unwrap_or(10);
    
    // Track sibling counts per directory
    let mut dir_file_counts: HashMap<PathBuf, (usize, HashMap<String, usize>)> = HashMap::new();
    
    // First pass: count files per directory
    for entry in WalkDir::new(root)
        .max_depth(max_d)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(parent) = entry.path().parent() {
                let ext = entry.path()
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                
                let (count, ext_counts) = dir_file_counts
                    .entry(parent.to_path_buf())
                    .or_insert((0, HashMap::new()));
                *count += 1;
                *ext_counts.entry(ext).or_insert(0) += 1;
            }
        }
    }
    
    // Second pass: collect documents
    for entry in WalkDir::new(root)
        .max_depth(max_d)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        
        let path = entry.path();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        
        let doc_type = DocumentType::from_extension(&ext);
        
        // Skip code files and unknown types for organization
        if !doc_type.is_organizable() {
            continue;
        }
        
        let metadata = entry.metadata().ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = metadata
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: DateTime<Local> = t.into();
                dt.format("%Y-%m-%d %H:%M:%S").to_string()
            })
            .unwrap_or_else(|| "unknown".to_string());
        
        let parent = path.parent().unwrap_or(Path::new(""));
        let depth = path.components().count() - root.components().count();
        
        let (siblings, ext_counts) = dir_file_counts
            .get(&parent.to_path_buf())
            .cloned()
            .unwrap_or((0, HashMap::new()));
        
        let similar = ext_counts.get(&ext).cloned().unwrap_or(0);
        
        documents.push(DiscoveredDocument {
            path: path.to_string_lossy().to_string(),
            name: path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            extension: ext,
            doc_type,
            size_bytes: size,
            modified,
            parent_dir: parent.to_string_lossy().to_string(),
            depth,
            siblings_count: siblings,
            similar_siblings: similar,
        });
    }
    
    println!("[FILE_INTEL] Scanned {} organizable documents", documents.len());
    Ok(documents)
}

/// Analyze documents and generate organization suggestions
pub fn generate_suggestions(documents: &[DiscoveredDocument], preferences: &UserPreferences) -> Vec<OrganizationSuggestion> {
    let mut suggestions = Vec::new();
    
    for doc in documents {
        // Skip dismissed files
        if preferences.dismissed_suggestions.contains(&doc.path) {
            continue;
        }
        
        // Analyze the document
        if let Some(suggestion) = analyze_document(doc, preferences) {
            suggestions.push(suggestion);
        }
    }
    
    // Sort by confidence (highest first)
    suggestions.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    
    suggestions
}

/// Analyze a single document and maybe suggest an action
fn analyze_document(doc: &DiscoveredDocument, preferences: &UserPreferences) -> Option<OrganizationSuggestion> {
    let name_lower = doc.name.to_lowercase();
    let parent_lower = doc.parent_dir.to_lowercase();
    
    // Check if it's in project directory (has code files, git, etc.)
    if is_project_directory(&doc.parent_dir) {
        return None; // Leave project files alone
    }
    
    // Check if file is in Downloads
    if parent_lower.contains("downloads") {
        return suggest_from_downloads(doc, preferences);
    }
    
    // Check for resume/CV
    if name_lower.contains("resume") || name_lower.contains("cv") {
        return Some(OrganizationSuggestion {
            file_path: doc.path.clone(),
            file_name: doc.name.clone(),
            action: SuggestionAction::Move {
                to_path: get_documents_path("Career/Resumes"),
            },
            confidence: 0.9,
            reason: "Resume detected - keep with other career documents".to_string(),
            category: "Resumes".to_string(),
        });
    }
    
    // Check for receipts
    if name_lower.contains("receipt") || name_lower.contains("invoice") {
        return Some(OrganizationSuggestion {
            file_path: doc.path.clone(),
            file_name: doc.name.clone(),
            action: SuggestionAction::Move {
                to_path: get_documents_path("Finance/Receipts"),
            },
            confidence: 0.85,
            reason: "Receipt/invoice detected - organize with financial documents".to_string(),
            category: "Receipts".to_string(),
        });
    }
    
    // Check for poorly named files
    if is_poorly_named(&doc.name) {
        return Some(OrganizationSuggestion {
            file_path: doc.path.clone(),
            file_name: doc.name.clone(),
            action: SuggestionAction::Rename {
                new_name: suggest_better_name(doc),
            },
            confidence: 0.7,
            reason: "Generic filename - consider a more descriptive name".to_string(),
            category: "Naming".to_string(),
        });
    }
    
    // File is in a good place
    None
}

/// Check if directory is a code project (don't move files from here)
fn is_project_directory(path: &str) -> bool {
    let dir = Path::new(path);
    
    // Check for project indicators
    let indicators = [
        "package.json",
        "Cargo.toml",
        "requirements.txt",
        "setup.py",
        "pom.xml",
        "build.gradle",
        ".git",
        "node_modules",
        ".vscode",
        "Makefile",
        "CMakeLists.txt",
    ];
    
    for indicator in indicators {
        if dir.join(indicator).exists() {
            return true;
        }
    }
    
    // Check parent directories too
    if let Some(parent) = dir.parent() {
        for indicator in &indicators[..3] {  // Check main project files only
            if parent.join(indicator).exists() {
                return true;
            }
        }
    }
    
    false
}

/// Suggest what to do with files in Downloads
fn suggest_from_downloads(doc: &DiscoveredDocument, _preferences: &UserPreferences) -> Option<OrganizationSuggestion> {
    let name_lower = doc.name.to_lowercase();
    
    // Determine category based on content/name
    let (category, dest) = if name_lower.contains("resume") || name_lower.contains("cv") {
        ("Resumes", "Career/Resumes")
    } else if name_lower.contains("receipt") || name_lower.contains("invoice") || name_lower.contains("statement") {
        ("Finance", "Finance/Receipts")
    } else if name_lower.contains("report") || name_lower.contains("homework") || name_lower.contains("assignment") {
        ("School", "School")
    } else if name_lower.contains("contract") || name_lower.contains("agreement") || name_lower.contains("legal") {
        ("Legal", "Legal")
    } else {
        // Generic suggestion based on file type
        match doc.doc_type {
            DocumentType::Word => ("Documents", "Documents/Word"),
            DocumentType::Excel => ("Spreadsheets", "Documents/Spreadsheets"),
            DocumentType::PowerPoint => ("Presentations", "Documents/Presentations"),
            DocumentType::PDF => ("PDFs", "Documents/PDFs"),
            _ => ("Misc", "Documents/Misc"),
        }
    };
    
    Some(OrganizationSuggestion {
        file_path: doc.path.clone(),
        file_name: doc.name.clone(),
        action: SuggestionAction::Move {
            to_path: get_documents_path(dest),
        },
        confidence: 0.75,
        reason: format!("File in Downloads - move to {} folder?", category),
        category: category.to_string(),
    })
}

/// Check if a filename is poorly named (generic)
fn is_poorly_named(name: &str) -> bool {
    let generic_patterns = [
        "document", "untitled", "new ", "copy of", "document1", "document2",
        "file", "download", "attachment", "scan", "img_", "dsc_",
    ];
    
    let _name_lower = name.to_lowercase();
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    
    // Check for generic patterns
    for pattern in generic_patterns {
        if stem.starts_with(pattern) || stem == pattern {
            return true;
        }
    }
    
    // Check for just numbers
    if stem.chars().all(|c| c.is_numeric() || c == '-' || c == '_') {
        return true;
    }
    
    // Check for very short names
    if stem.len() <= 2 {
        return true;
    }
    
    false
}

/// Suggest a better name for a file
fn suggest_better_name(doc: &DiscoveredDocument) -> String {
    let date = chrono::Local::now().format("%Y-%m-%d");
    let ext = &doc.extension;
    
    // Try to extract something meaningful
    let base = match doc.doc_type {
        DocumentType::Word => "document",
        DocumentType::Excel => "spreadsheet",
        DocumentType::PowerPoint => "presentation",
        DocumentType::PDF => "document",
        _ => "file",
    };
    
    format!("{}-{}.{}", date, base, ext)
}

/// Get the user's Documents path
fn get_documents_path(subpath: &str) -> String {
    #[cfg(windows)]
    {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            return format!("{}\\Documents\\{}", userprofile, subpath.replace('/', "\\"));
        }
    }
    
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/Documents/{}", home, subpath);
        }
    }
    
    format!("~/Documents/{}", subpath)
}

/// Detect naming patterns from existing files
pub fn detect_naming_patterns(documents: &[DiscoveredDocument]) -> Vec<NamingPattern> {
    let mut patterns: HashMap<PatternType, (usize, String)> = HashMap::new();
    
    for doc in documents {
        let stem = Path::new(&doc.name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        
        let pattern = classify_name_pattern(&stem);
        
        let entry = patterns.entry(pattern.clone()).or_insert((0, stem.clone()));
        entry.0 += 1;
    }
    
    let total = documents.len().max(1) as f32;
    
    patterns
        .into_iter()
        .map(|(pattern_type, (count, example))| NamingPattern {
            pattern_type,
            example,
            frequency: count,
            confidence: count as f32 / total,
        })
        .collect()
}

/// Classify what naming pattern a filename uses
fn classify_name_pattern(stem: &str) -> PatternType {
    // Check for date prefix (YYYY-MM-DD or YYYYMMDD at start)
    let date_prefix_regex = regex::Regex::new(r"^\d{4}[-_]?\d{2}[-_]?\d{2}").ok();
    if let Some(re) = &date_prefix_regex {
        if re.is_match(stem) {
            return PatternType::DatePrefix;
        }
    }
    
    // Check for date suffix
    let date_suffix_regex = regex::Regex::new(r"\d{4}[-_]?\d{2}[-_]?\d{2}$").ok();
    if let Some(re) = &date_suffix_regex {
        if re.is_match(stem) {
            return PatternType::DateSuffix;
        }
    }
    
    // Check for version numbers (v1, v2, _v3, etc.)
    let version_regex = regex::Regex::new(r"[_-]?v\d+").ok();
    if let Some(re) = &version_regex {
        if re.is_match(stem) {
            return PatternType::VersionNumbered;
        }
    }
    
    // Check for category-first (word-word-word pattern)
    if stem.contains('-') || stem.contains('_') {
        let parts: Vec<&str> = stem.split(|c| c == '-' || c == '_').collect();
        if parts.len() >= 2 && parts.iter().all(|p| p.chars().all(|c| c.is_alphabetic())) {
            return PatternType::CategoryFirst;
        }
    }
    
    // Check for semantic (multiple readable words)
    if stem.len() > 15 && stem.chars().filter(|c| c.is_alphabetic()).count() > 10 {
        return PatternType::Semantic;
    }
    
    // Default: unstructured
    PatternType::Unstructured
}

/// Calculate scan statistics
pub fn calculate_statistics(documents: &[DiscoveredDocument]) -> ScanStatistics {
    let mut by_type: HashMap<String, usize> = HashMap::new();
    let mut by_location: HashMap<String, usize> = HashMap::new();
    let mut unorganized = 0;
    
    for doc in documents {
        // Count by type
        let type_name = format!("{:?}", doc.doc_type);
        *by_type.entry(type_name).or_insert(0) += 1;
        
        // Count by parent directory
        *by_location.entry(doc.parent_dir.clone()).or_insert(0) += 1;
        
        // Check if unorganized
        if is_poorly_named(&doc.name) || doc.parent_dir.to_lowercase().contains("downloads") {
            unorganized += 1;
        }
    }
    
    // Calculate naming consistency score
    let patterns = detect_naming_patterns(documents);
    let naming_score = if patterns.is_empty() {
        50.0
    } else {
        let best_pattern = patterns.iter()
            .filter(|p| p.pattern_type != PatternType::Unstructured)
            .max_by(|a, b| a.frequency.cmp(&b.frequency));
        
        match best_pattern {
            Some(p) => (p.confidence * 100.0).min(100.0),
            None => 30.0,  // Mostly unstructured
        }
    };
    
    ScanStatistics {
        total_documents: documents.len(),
        by_type,
        by_location,
        potential_duplicates: 0,  // TODO: implement duplicate detection
        unorganized_count: unorganized,
        naming_score,
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_document_type_detection() {
        assert_eq!(DocumentType::from_extension("docx"), DocumentType::Word);
        assert_eq!(DocumentType::from_extension("PDF"), DocumentType::PDF);
        assert_eq!(DocumentType::from_extension("py"), DocumentType::Code);
        assert_eq!(DocumentType::from_extension("xyz"), DocumentType::Unknown);
    }
    
    #[test]
    fn test_poorly_named() {
        assert!(is_poorly_named("Document1.docx"));
        assert!(is_poorly_named("Untitled.pdf"));
        assert!(is_poorly_named("Copy of Report.docx"));
        assert!(!is_poorly_named("2026-02-10-meeting-notes.md"));
        assert!(!is_poorly_named("quarterly-budget-report.xlsx"));
    }
    
    #[test]
    fn test_pattern_classification() {
        assert_eq!(classify_name_pattern("2026-02-10-report"), PatternType::DatePrefix);
        assert_eq!(classify_name_pattern("report-2026-02-10"), PatternType::DateSuffix);
        assert_eq!(classify_name_pattern("report-v3"), PatternType::VersionNumbered);
        assert_eq!(classify_name_pattern("notes-meeting-budget"), PatternType::CategoryFirst);
    }
}
