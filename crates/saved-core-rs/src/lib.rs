//! # SAVED Core Library
//!
//! A private, end-to-end encrypted personal vault that syncs notes, messages, and files
//! across your own devices using P2P networking with libp2p.
//!
//! ## Key Features
//!
//! * P2P by default (mDNS/LAN → direct → DCUtR → relay fallback)
//! * End-to-end encryption with account-scoped vault key
//! * Append-only event log + CRDT semantics for edits/deletes
//! * Content-addressed encrypted chunk store for files; dedup within the account
//! * Optional zero-knowledge Cloud Backup + Relay as a paid add-on
//! * Minimal to no servers required for users who don't opt in

pub mod crypto;
pub mod storage;
pub mod events;
pub mod sync;
pub mod types;
pub mod error;
pub mod protobuf;

// Re-export main types for convenience
pub use types::*;
pub use error::{Error, Result};

/// Main entry point for creating or opening an account
pub async fn create_or_open_account(config: Config) -> Result<AccountHandle> {
    AccountHandle::create_or_open(config).await
}