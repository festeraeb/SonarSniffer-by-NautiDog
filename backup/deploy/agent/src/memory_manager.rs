//! Memory Manager — Persistent agent state across mission phases.
//!
//! Agents need to remember what they've done, what they've learned, and what
//! context is relevant across tool calls, sessions, and mission phases.
//! This module provides:
//!
//! - Short-term memory (current session context, trimmed for token budget)
//! - Long-term memory (persisted to disk, searchable via nautivecs)
//! - Mission memory (scoped to a specific SAR case, shared across agents)
//! - Lessons learned (cross-mission knowledge that improves future performance)

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A single memory entry.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MemoryEntry {
    /// Unique ID
    pub id: u64,
    /// Unix timestamp when created
    pub timestamp: u64,
    /// Content of the memory
    pub content: String,
    /// Tags for categorization and search
    pub tags: Vec<String>,
    /// Source (which agent/tool created this)
    pub source: String,
    /// Relevance score (decays over time, boosted on access)
    pub relevance: f32,
    /// Mission ID this memory belongs to (None = global)
    pub mission_id: Option<String>,
}

/// Short-term memory — bounded ring buffer for current session.
pub struct ShortTermMemory {
    /// Recent entries (FIFO, oldest dropped when full)
    entries: VecDeque<MemoryEntry>,
    /// Maximum entries to keep
    max_entries: usize,
    /// Maximum total characters (for token budget estimation)
    max_chars: usize,
    /// Current total character count
    total_chars: usize,
}

impl ShortTermMemory {
    pub fn new(max_entries: usize, max_chars: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
            max_chars,
            total_chars: 0,
        }
    }

    /// Add a new entry. Evicts oldest if over capacity.
    pub fn push(&mut self, content: String, tags: Vec<String>, source: String) {
        let entry = MemoryEntry {
            id: next_id(),
            timestamp: now_unix(),
            content: content.clone(),
            tags,
            source,
            relevance: 1.0,
            mission_id: None,
        };

        self.total_chars += content.len();
        self.entries.push_back(entry);

        // Evict oldest until within limits
        while self.entries.len() > self.max_entries || self.total_chars > self.max_chars {
            if let Some(removed) = self.entries.pop_front() {
                self.total_chars = self.total_chars.saturating_sub(removed.content.len());
            } else {
                break;
            }
        }
    }

    /// Get all entries as a formatted context string for injection into prompts.
    pub fn as_context(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }

        let mut ctx = String::from("[MEMORY CONTEXT]\n");
        for entry in self.entries.iter().rev().take(10) {
            ctx.push_str(&format!("- [{}] {}\n", entry.tags.join(","), &entry.content[..entry.content.len().min(200)]));
        }
        ctx
    }

    /// Search short-term memory by tag.
    pub fn search_by_tag(&self, tag: &str) -> Vec<&MemoryEntry> {
        self.entries.iter()
            .filter(|e| e.tags.iter().any(|t| t.contains(tag)))
            .collect()
    }

    /// Get entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_chars = 0;
    }
}

/// Long-term memory — persisted to disk as JSONL, searchable.
pub struct LongTermMemory {
    /// Path to the memory file
    path: PathBuf,
    /// In-memory index for fast search (tag → entry IDs)
    tag_index: std::collections::HashMap<String, Vec<u64>>,
    /// All entries (loaded on init)
    entries: Vec<MemoryEntry>,
}

impl LongTermMemory {
    /// Load or create long-term memory at the given path.
    pub fn load(path: &Path) -> Self {
        let entries = if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    content.lines()
                        .filter_map(|line| serde_json::from_str::<MemoryEntry>(line).ok())
                        .collect()
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let mut tag_index: std::collections::HashMap<String, Vec<u64>> = std::collections::HashMap::new();
        for entry in &entries {
            for tag in &entry.tags {
                tag_index.entry(tag.clone()).or_default().push(entry.id);
            }
        }

        Self {
            path: path.to_path_buf(),
            tag_index,
            entries,
        }
    }

    /// Store a new memory entry (appends to file).
    pub fn store(&mut self, content: String, tags: Vec<String>, source: String, mission_id: Option<String>) {
        let entry = MemoryEntry {
            id: next_id(),
            timestamp: now_unix(),
            content,
            tags: tags.clone(),
            source,
            relevance: 1.0,
            mission_id,
        };

        // Update index
        for tag in &tags {
            self.tag_index.entry(tag.clone()).or_default().push(entry.id);
        }

        // Append to file
        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, "{}", line)
                });
        }

        self.entries.push(entry);
    }

    /// Search by tags (OR logic — matches any tag).
    pub fn search(&self, query_tags: &[&str], limit: usize) -> Vec<&MemoryEntry> {
        let mut matching_ids: Vec<u64> = Vec::new();
        for tag in query_tags {
            if let Some(ids) = self.tag_index.get(*tag) {
                matching_ids.extend(ids);
            }
        }
        matching_ids.sort_unstable();
        matching_ids.dedup();

        // Return most recent matches
        let mut results: Vec<&MemoryEntry> = self.entries.iter()
            .filter(|e| matching_ids.contains(&e.id))
            .collect();
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        results.truncate(limit);
        results
    }

    /// Search by content substring.
    pub fn search_content(&self, query: &str, limit: usize) -> Vec<&MemoryEntry> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<&MemoryEntry> = self.entries.iter()
            .filter(|e| e.content.to_lowercase().contains(&query_lower))
            .collect();
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        results.truncate(limit);
        results
    }

    /// Get all entries for a specific mission.
    pub fn mission_entries(&self, mission_id: &str) -> Vec<&MemoryEntry> {
        self.entries.iter()
            .filter(|e| e.mission_id.as_deref() == Some(mission_id))
            .collect()
    }

    /// Get total entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Mission-scoped memory — shared across all agents working on the same case.
pub struct MissionMemory {
    pub mission_id: String,
    pub long_term: LongTermMemory,
}

impl MissionMemory {
    /// Create or load mission memory for a specific case.
    pub fn for_mission(mission_id: &str, base_dir: &Path) -> Self {
        let path = base_dir.join(format!("mission_{}.jsonl", mission_id));
        Self {
            mission_id: mission_id.to_string(),
            long_term: LongTermMemory::load(&path),
        }
    }

    /// Record a finding (shared across all agents on this mission).
    pub fn record_finding(&mut self, content: String, tags: Vec<String>, source: String) {
        self.long_term.store(content, tags, source, Some(self.mission_id.clone()));
    }

    /// Get all findings for this mission.
    pub fn all_findings(&self) -> Vec<&MemoryEntry> {
        self.long_term.mission_entries(&self.mission_id)
    }

    /// Generate a mission summary from accumulated findings.
    pub fn summary(&self) -> String {
        let findings = self.all_findings();
        if findings.is_empty() {
            return format!("Mission {}: No findings recorded yet.", self.mission_id);
        }

        let mut summary = format!("Mission {} Summary ({} findings):\n", self.mission_id, findings.len());
        for (i, f) in findings.iter().enumerate().take(20) {
            summary.push_str(&format!("  {}. [{}] {}\n", i + 1, f.tags.join(","), &f.content[..f.content.len().min(100)]));
        }
        summary
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn next_id() -> u64 {
    // Simple monotonic ID using timestamp + random bits
    let ts = now_unix();
    let rand_bits = (std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64) & 0xFFFF;
    (ts << 16) | rand_bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_short_term_memory() {
        let mut stm = ShortTermMemory::new(5, 1000);

        stm.push("Found metal signature at 41.5N 82.3W".to_string(), vec!["detection".to_string()], "spectral_unmixer".to_string());
        stm.push("Curvelet response high in sector 7".to_string(), vec!["filter".to_string()], "curvelet_filter".to_string());

        assert_eq!(stm.len(), 2);
        let ctx = stm.as_context();
        assert!(ctx.contains("metal signature"));
        assert!(ctx.contains("Curvelet response"));
    }

    #[test]
    fn test_short_term_eviction() {
        let mut stm = ShortTermMemory::new(3, 500);

        for i in 0..5 {
            stm.push(format!("Entry {}", i), vec!["test".to_string()], "test".to_string());
        }

        // Should only keep last 3
        assert_eq!(stm.len(), 3);
        let ctx = stm.as_context();
        assert!(ctx.contains("Entry 4"));
        assert!(ctx.contains("Entry 3"));
        assert!(!ctx.contains("Entry 0"));
    }

    #[test]
    fn test_long_term_memory() {
        let tmp = std::env::temp_dir().join("test_ltm.jsonl");
        let _ = std::fs::remove_file(&tmp);

        let mut ltm = LongTermMemory::load(&tmp);
        ltm.store("Wreck confirmed at coordinates X".to_string(), vec!["wreck".to_string(), "confirmed".to_string()], "classifier".to_string(), None);
        ltm.store("False positive at coordinates Y".to_string(), vec!["geological".to_string(), "dismissed".to_string()], "classifier".to_string(), None);

        let results = ltm.search(&["wreck"], 10);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("confirmed"));

        // Reload from disk
        let ltm2 = LongTermMemory::load(&tmp);
        assert_eq!(ltm2.len(), 2);

        let _ = std::fs::remove_file(&tmp);
    }
}
