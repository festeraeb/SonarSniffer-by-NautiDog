//! Shared error type for signal / pipeline modules (Lane B contract).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{0}")]
    Msg(String),
    #[error("dimension mismatch: {0}")]
    Dim(String),
    #[error("invalid shift peak at ({row}, {col})")]
    InvalidPeak { row: usize, col: usize },
}

impl From<String> for EngineError {
    fn from(s: String) -> Self {
        Self::Msg(s)
    }
}

impl From<&str> for EngineError {
    fn from(s: &str) -> Self {
        Self::Msg(s.to_string())
    }
}
