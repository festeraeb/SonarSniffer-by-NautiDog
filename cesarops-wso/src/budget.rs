use crate::search::WebFinding;

/// Token budget allocator with confidence weighting
pub struct TokenBudgetAllocator {
    total_budget: usize,
}

impl TokenBudgetAllocator {
    /// Create a new allocator with the given total token budget
    pub fn new(total_budget: usize) -> Self {
        Self { total_budget }
    }

    /// Allocate tokens to findings based on confidence scores
    pub fn allocate(&self, findings: &[WebFinding]) -> Vec<usize> {
        if findings.is_empty() {
            return Vec::new();
        }

        // Calculate total confidence
        let total_confidence: f32 = findings.iter().map(|f| f.confidence).sum();
        
        if total_confidence == 0.0 {
            // Equal distribution if no confidence scores
            let per_finding = self.total_budget / findings.len();
            return vec![per_finding; findings.len()];
        }

        // Sort findings by confidence descending
        let mut sorted_indices: Vec<usize> = (0..findings.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            findings[b].confidence.partial_cmp(&findings[a].confidence).unwrap()
        });

        // Allocate tokens proportionally
        let mut allocations = vec![0usize; findings.len()];
        let mut remaining_budget = self.total_budget;

        for (i, &idx) in sorted_indices.iter().enumerate() {
            let confidence = findings[idx].confidence;
            
            // Calculate proportional allocation
            let alloc = (remaining_budget as f32 * confidence / total_confidence) as usize;
            
            // Ensure we don't exceed remaining budget
            let alloc = alloc.min(remaining_budget);
            
            allocations[idx] = alloc;
            remaining_budget -= alloc;
        }

        // Distribute any remaining budget to highest confidence items
        if remaining_budget > 0 && !sorted_indices.is_empty() {
            allocations[sorted_indices[0]] += remaining_budget;
        }

        allocations
    }
}
