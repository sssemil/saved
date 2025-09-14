use crate::error::Result;
use crate::events::Op;
use crate::types::{Message, MessageId};
use async_trait::async_trait;

/// Storage trait defining the interface for all storage backends
#[async_trait]
pub trait Storage: Send + Sync {
    /// Initialize the storage backend
    async fn init(&mut self) -> Result<()>;

    /// Store an operation
    async fn store_operation(&self, operation: &Op) -> Result<()>;

    /// Get all operations
    async fn get_all_operations(&self) -> Result<Vec<Op>>;

    /// Get operations for a specific device
    async fn get_device_operations(&self, device_id: &[u8; 32]) -> Result<Vec<Op>>;

    /// Store a chunk of data
    async fn store_chunk(&self, hash: &[u8; 32], data: &[u8]) -> Result<()>;

    /// Retrieve a chunk of data
    async fn get_chunk(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>>;

    /// Check if a chunk exists
    async fn has_chunk(&self, hash: &[u8; 32]) -> Result<bool>;

    /// Get all messages (materialized view)
    async fn get_all_messages(&self) -> Result<Vec<Message>>;

    /// Get a specific message by ID
    async fn get_message(&self, message_id: &MessageId) -> Result<Option<Message>>;

    /// Store a message (materialized view)
    async fn store_message(&self, message: &Message) -> Result<()>;

    /// Delete a message
    async fn delete_message(&self, message_id: &MessageId) -> Result<()>;

    /// Store the account key (encrypted with passphrase)
    async fn store_account_key(&self, encrypted_account_key: &[u8]) -> Result<()>;

    /// Retrieve the encrypted account key
    async fn get_account_key(&self) -> Result<Option<Vec<u8>>>;

    /// Store the vault key (encrypted with passphrase)
    async fn store_vault_key(&self, encrypted_vault_key: &[u8]) -> Result<()>;

    /// Retrieve the encrypted vault key
    async fn get_vault_key(&self) -> Result<Option<Vec<u8>>>;

    /// Get storage statistics
    async fn get_stats(&self) -> Result<StorageStats>;
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub operation_count: usize,
    pub message_count: usize,
    pub chunk_count: usize,
    pub total_size: u64,
}
