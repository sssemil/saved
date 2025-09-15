//! Error types for the SAVED core library

use thiserror::Error;

/// Main error type for the SAVED core library
#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Cryptography error: {0}")]
    Crypto(String),

    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Device linking error: {0}")]
    DeviceLinking(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Sync error: {0}")]
    Sync(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Account not found")]
    AccountNotFound,

    #[error("Device not authorized")]
    DeviceNotAuthorized,

    #[error("Message not found")]
    MessageNotFound,

    #[error("Operation failed: {0}")]
    OperationFailed(String),

    #[error("Channel send error: {0}")]
    ChannelSend(#[from] std::sync::mpsc::SendError<crate::types::Event>),

    #[error("Tokio channel send error: {0}")]
    TokioChannelSend(#[from] tokio::sync::mpsc::error::SendError<crate::types::Event>),

    #[error("Walkdir error: {0}")]
    Walkdir(#[from] walkdir::Error),
}

/// Result type alias for the SAVED core library
pub type Result<T> = std::result::Result<T, Error>;
