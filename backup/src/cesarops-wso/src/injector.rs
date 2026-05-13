use tracing::info;

use crate::search::WebFinding;

/// Context injector for steering engine integration
pub struct ContextInjector;

impl ContextInjector {
    pub fn new() -> Self {
        Self
    }

    /// Build a context fragment from a web finding
    pub fn build_context_fragment(&self, finding: &WebFinding, max_tokens: usize) -> String {
        // Estimate tokens (rough approximation: 1 token ≈ 4 characters)
        let max_chars = max_tokens * 4;
        
        let mut fragment = format!(
            "## Web Finding: {}\nURL: {}\nSnippet: {}\n\n",
            finding.title, finding.url, finding.snippet
        );
        
        // Add content if available and within token budget
        if !finding.content.is_empty() && finding.content.len() <= max_chars {
            fragment.push_str(&finding.content);
        } else if finding.content.len() > max_chars {
            fragment.push_str(&finding.content[..max_chars]);
            fragment.push_str("... [truncated]");
        }
        
        fragment
    }

    /// Inject context fragments into the steering context
    pub fn inject(&self, steering_context: &mut String, fragments: &[String]) {
        if fragments.is_empty() {
            return;
        }
        
        // Add separator if context already has content
        if !steering_context.is_empty() {
            steering_context.push_str("\n\n---\n\n");
        }
        
        // Append all fragments
        for fragment in fragments {
            steering_context.push_str(fragment);
            steering_context.push_str("\n\n");
        }
        
        info!("Injected {} context fragments into steering", fragments.len());
    }
}
