use reqwest::Client;
use scraper::{Html, Selector};
use anyhow::Result;
use tracing::debug;

use crate::errors::{WsoError, WsoResult};

/// HTML content extractor
pub struct WebExtractor;

impl WebExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Fetch and extract clean text from a URL
    pub async fn fetch_and_extract(&self, url: &str, client: &Client) -> WsoResult<String> {
        // Check robots.txt (simplified implementation)
        if !self.is_allowed_by_robots(url).await? {
            return Err(WsoError::ExtractionFailed(format!(
                "URL {} disallowed by robots.txt", url
            )));
        }

        // Fetch the page
        let resp = client.get(url).send().await?;
        
        if !resp.status().is_success() {
            return Err(WsoError::ExtractionFailed(format!(
                "Failed to fetch {}: status {}", url, resp.status()
            )));
        }

        let html = resp.text().await?;
        
        // Extract clean text
        let text = self.extract_clean_text(&html)?;
        
        Ok(text)
    }

    /// Extract clean text from HTML content
    pub fn extract_clean_text(&self, html: &str) -> WsoResult<String> {
        let document = Html::parse_document(html);
        
        // Select body content
        let body_selector = Selector::parse("body").map_err(|e| {
            WsoError::ExtractionFailed(format!("Invalid selector: {}", e))
        })?;
        
        let body = document.select(&body_selector).next().ok_or_else(|| {
            WsoError::ExtractionFailed("No body element found".to_string())
        })?;
        
        // Remove scripts and styles
        let script_selector = Selector::parse("script, style").map_err(|e| {
            WsoError::ExtractionFailed(format!("Invalid selector: {}", e))
        })?;
        
        // Collect text while removing unwanted elements
        let mut text_parts = Vec::new();
        for node in body.descendants() {
            if node.value().is_text() {
                if let Some(text) = node.value().as_text() {
                    text_parts.push(text.to_string());
                }
            }
        }
        
        // Join and clean up whitespace
        let full_text = text_parts.join(" ");
        let clean_text = self.clean_whitespace(&full_text);
        
        debug!("Extracted {} bytes of text from URL", clean_text.len());
        
        Ok(clean_text)
    }

    /// Clean up whitespace in extracted text
    fn clean_whitespace(&self, text: &str) -> String {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Check if URL is allowed by robots.txt (simplified)
    async fn is_allowed_by_robots(&self, url: &str) -> WsoResult<bool> {
        // In a production implementation, this would fetch and parse robots.txt
        // For now, we allow all URLs (simplified)
        Ok(true)
    }
}
