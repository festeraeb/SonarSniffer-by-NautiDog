use thiserror::Error;
use reqwest::Error as ReqwestError;

/// Result type alias for WSO operations
pub type WsoResult<T> = Result<T, WsoError>;

/// Error types for the Web Search Oracle
#[derive(Error, Debug)]
pub enum WsoError {
    #[error("Search engine unavailable: {0}")]
    SearchEngineUnavailable(String),
    
    #[error("Content extraction failed: {0}")]
    ExtractionFailed(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Token budget exceeded")]
    TokenBudgetExceeded,
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("Network error: {0}")]
    NetworkError(#[from] ReqwestError),
    
    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}
