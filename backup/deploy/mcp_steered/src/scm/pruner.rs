//! Context Pruner — strips context to fit low-VRAM hardware.
//!
//! On 8GB cards (1060/1070/P1000), we can't afford to send the entire
//! conversation history + all RSU outputs to the model. The pruner
//! retains only what's essential:
//! - Current RSU
//! - Immediate parent RSU output (for continuity)
//! - Global steering rules
//! - Active corrections
//!
//! Everything else gets dropped to prevent context bloat from
//! crashing Pascal-era cards.

use super::drift::HardwareProfile;

/// Prunes context to fit within hardware constraints
pub struct ContextPruner;

impl ContextPruner {
    /// Aggressively prune the working context for low-VRAM hardware.
    /// Retains only the 2 most recent RSU outputs + global steering.
    pub fn prune_for_hardware(context: &mut String, profile: &HardwareProfile) {
        if !profile.aggressive_pruning {
            return; // High-VRAM cards keep full context
        }

        // For 8GB cards: keep only essential segments
        *context = Self::retain_essential_segments(context, 2);
    }

    /// Keep only the N most recent segment outputs + any steering/correction blocks.
    fn retain_essential_segments(context: &str, max_segments: usize) -> String {
        let mut result = String::new();
        let mut segments_kept = 0;

        // Always keep steering rules and corrections (they're small and critical)
        for line in context.lines() {
            if line.contains("[CRITICAL: PREVIOUS HUMAN CORRECTION")
                || line.contains("## Grounding Rules")
                || line.contains("[STEERING ALERT]")
                || line.starts_with("SOURCE:")
            {
                result.push_str(line);
                result.push('\n');
            }
        }

        // Keep the most recent N segment outputs (scan from end)
        let segments: Vec<&str> = context.split("### SEGMENT OUTPUT").collect();
        let start = if segments.len() > max_segments + 1 {
            segments.len() - max_segments
        } else {
            1 // Skip the first split (before any segment marker)
        };

        for segment in &segments[start..] {
            if segments_kept >= max_segments {
                break;
            }
            result.push_str("### SEGMENT OUTPUT");
            result.push_str(segment);
            segments_kept += 1;
        }

        // If nothing was kept (no segment markers), return a truncated version
        if result.is_empty() {
            let max_chars = 3000; // ~750 tokens — safe for 8GB with 4096 context
            return context.chars().rev().take(max_chars).collect::<String>().chars().rev().collect();
        }

        result
    }

    /// Estimate token count (rough: 1 token ≈ 4 chars for English/code)
    pub fn estimate_tokens(text: &str) -> usize {
        text.len() / 4
    }

    /// Check if context exceeds the budget and needs pruning
    pub fn exceeds_budget(context: &str, budget_tokens: usize) -> bool {
        Self::estimate_tokens(context) > budget_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pruning_keeps_corrections() {
        let context = "Some old stuff\n[CRITICAL: PREVIOUS HUMAN CORRECTION]: use 0.3\nMore old stuff\n## Grounding Rules\nDon't hallucinate";
        let profile = HardwareProfile::low_vram();
        let mut ctx = context.to_string();
        ContextPruner::prune_for_hardware(&mut ctx, &profile);
        assert!(ctx.contains("[CRITICAL: PREVIOUS HUMAN CORRECTION]"));
        assert!(ctx.contains("## Grounding Rules"));
    }

    #[test]
    fn test_no_pruning_on_high_vram() {
        let context = "Full context with lots of history that should be preserved";
        let profile = HardwareProfile::high_vram();
        let mut ctx = context.to_string();
        ContextPruner::prune_for_hardware(&mut ctx, &profile);
        assert_eq!(ctx, context); // Unchanged
    }

    #[test]
    fn test_token_estimation() {
        assert_eq!(ContextPruner::estimate_tokens("hello world"), 2); // 11 chars / 4
    }
}
