//! Failover endpoint pool — try candidates in priority order; re-probe on failure.
//!
//! Env: `{ROLE}_URLS` comma-separated, or `{ROLE}_URL` single, or built-in defaults.

use std::sync::Arc;
use tokio::sync::RwLock;

const DEFAULT_SCOUT: &str =
    "http://10.0.0.201:5570,http://10.0.0.200:5570,http://127.0.0.1:5570";
const DEFAULT_VALIDATOR: &str =
    "http://10.0.0.201:5572,http://10.0.0.200:5572,http://10.0.0.201:5571,http://10.0.0.200:5571,http://127.0.0.1:5572";
const DEFAULT_JITTER: &str =
    "http://10.0.0.61:8180,http://10.0.0.61:8080,http://10.0.0.201:8080,http://10.0.0.200:8080,http://127.0.0.1:8180";

pub struct EndpointPool {
    role: &'static str,
    candidates: Vec<String>,
    active: Arc<RwLock<Option<String>>>,
    client: reqwest::Client,
}

impl EndpointPool {
    pub fn new(role: &'static str, env_single: &str, env_list: &str, defaults: &str) -> Self {
        let candidates = parse_urls(
            std::env::var(env_list)
                .ok()
                .or_else(|| std::env::var(env_single).ok())
                .unwrap_or_else(|| defaults.to_string()),
        );
        Self {
            role,
            candidates,
            active: Arc::new(RwLock::new(None)),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap(),
        }
    }

    pub fn scout() -> Self {
        Self::new("scout", "SCOUT_URL", "SCOUT_URLS", DEFAULT_SCOUT)
    }

    pub fn validator() -> Self {
        Self::new(
            "validator",
            "VALIDATOR_URL",
            "VALIDATOR_URLS",
            DEFAULT_VALIDATOR,
        )
    }

    pub fn jitter() -> Self {
        Self::new("jitter", "JITTER_URL", "JITTER_URLS", DEFAULT_JITTER)
    }

    pub async fn active_base(&self) -> Option<String> {
        {
            let guard = self.active.read().await;
            if let Some(url) = guard.as_ref() {
                if self.probe(url).await {
                    return Some(url.clone());
                }
            }
        }
        self.refresh().await
    }

    pub async fn refresh(&self) -> Option<String> {
        for url in &self.candidates {
            if self.probe(url).await {
                let mut guard = self.active.write().await;
                if guard.as_deref() != Some(url.as_str()) {
                    tracing::info!(
                        "[{}] using {} (was {:?})",
                        self.role,
                        url,
                        guard.as_deref()
                    );
                }
                *guard = Some(url.clone());
                return Some(url.clone());
            }
        }
        let mut guard = self.active.write().await;
        *guard = None;
        tracing::warn!("[{}] no healthy endpoint in pool {:?}", self.role, self.candidates);
        None
    }

    pub async fn health(&self) -> bool {
        self.active_base().await.is_some()
    }

    pub async fn active_label(&self) -> String {
        self.active_base()
            .await
            .unwrap_or_else(|| format!("{}:offline", self.role))
    }

    async fn probe(&self, base: &str) -> bool {
        self.client
            .get(format!("{}/health", base.trim_end_matches('/')))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub fn http_client(&self) -> reqwest::Client {
        self.client.clone()
    }
}

fn parse_urls(raw: String) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
