use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Invalid glob pattern: {0}")]
    InvalidGlob(String),
    #[error("Invalid regex pattern: {0}")]
    InvalidRegex(String),
    #[error("Validation error: {0}")]
    Validation(String),
}
