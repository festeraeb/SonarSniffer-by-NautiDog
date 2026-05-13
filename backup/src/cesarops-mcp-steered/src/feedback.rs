//! Passive Feedback Logger — non-blocking POST to n8n webhook
//!
//! Every LLM decision is logged to n8n for optional human review.
//! The log is fire-and-forget (async, non-blocking) — never slows down the MCP response.
//!
//! n8n workflow picks these up and:
//! - Displays them on a dashboard (passive mode)
//! - Presents a form for correction (active mode, only for tune_parameters with low confidence)

use chrono::Utc;
use serde::Serialize;

/// A decision log entry sent to n8n
#[derive(Debug, Clone, Serialize)]
pub struct DecisionLog {
    pub id: String,
    pub timestamp: String,
    pub tool_name: String,
    pub query: String,
    pub response_summary: String,
    pub confidence: f32,
    pub fragments_used: usize,
    pub corrections_applied: usize,
    pub needs_review: bool,
    pub model: String,
}

/// The passive feedback logger — fire-and-forget to n8n webhook
pub struct FeedbackLogger {
    webhook_url: Option<String>,
    http: reqwest::Client,
}

impl FeedbackLogger {
    pub fn new(webhook_url: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        if let Some(ref url) = webhook_url {
            tracing::info!("FeedbackLogger: logging to n8n at {}", url);
        } else {
            tracing::info!("FeedbackLogger: no webhook URL — logging to stderr only");
        }

        Self { webhook_url, http }
    }

    /// Log a decision — non-blocking, fire-and-forget
    pub fn log_decision(
        &self,
        tool_name: &str,
        query: &str,
        response: &str,
        confidence: f32,
        fragments_used: usize,
        corrections_applied: usize,
        model: &str,
    ) {
        let entry = DecisionLog {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            tool_name: tool_name.to_string(),
            query: query.chars().take(200).collect(),
            response_summary: response.chars().take(300).collect(),
            confidence,
            fragments_used,
            corrections_applied,
            needs_review: confidence < 0.7,
            model: model.to_string(),
        };

        // Always log to stderr (visible in MCP server logs)
        if entry.needs_review {
            tracing::warn!(
                "LOW CONFIDENCE ({:.2}): {} — '{}' → flagged for review",
                confidence, tool_name, &entry.query[..entry.query.len().min(80)]
            );
        } else {
            tracing::info!(
                "Decision logged: {} ({:.2} confidence, {} fragments)",
                tool_name, confidence, fragments_used
            );
        }

        // Fire-and-forget POST to n8n webhook (if configured)
        if let Some(ref url) = self.webhook_url {
            let url = url.clone();
            let http = self.http.clone();
            tokio::spawn(async move {
                let _ = http.post(&url).json(&entry).send().await;
                // Intentionally ignore errors — this is passive logging
            });
        }
    }
}
