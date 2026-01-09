//! Error types for the Hivemind service.

use thiserror::Error;

/// Main error type for Hivemind operations.
#[derive(Error, Debug)]
pub enum HivemindError {
    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Rate limiting errors
    #[error("Rate limit error: {0}")]
    RateLimit(String),

    /// gRPC server errors
    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::transport::Error),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type alias for Hivemind operations.
pub type Result<T> = std::result::Result<T, HivemindError>;
