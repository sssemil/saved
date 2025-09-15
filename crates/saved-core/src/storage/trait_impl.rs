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

    /// Get all messages including deleted ones (for testing)
    async fn get_all_messages_including_deleted(&self) -> Result<Vec<Message>>;

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

    /// Store account key info (public metadata)
    async fn store_account_key_info(&self, key_info: &crate::crypto::AccountKeyInfo) -> Result<()>;

    /// Get account key info
    async fn get_account_key_info(&self) -> Result<Option<crate::crypto::AccountKeyInfo>>;

    /// Store account key for sharing with other devices (encrypted)
    async fn store_shared_account_key(&self, encrypted_account_key: &[u8]) -> Result<()>;

    /// Get shared account key (encrypted)
    async fn get_shared_account_key(&self) -> Result<Option<Vec<u8>>>;

    /// Store the vault key (encrypted with passphrase)
    async fn store_vault_key(&self, encrypted_vault_key: &[u8]) -> Result<()>;

    /// Retrieve the encrypted vault key
    async fn get_vault_key(&self) -> Result<Option<Vec<u8>>>;

    /// Store an authorized device certificate
    async fn store_authorized_device(&self, device_id: &str, device_cert: &[u8]) -> Result<()>;

    /// Get all authorized device certificates
    async fn get_authorized_devices(&self) -> Result<Vec<(String, Vec<u8>)>>;

    /// Remove device authorization
    async fn revoke_device_authorization(&self, device_id: &str) -> Result<()>;

    /// Check if a device is authorized
    async fn is_device_authorized(&self, device_id: &str) -> Result<bool>;

    /// Store device certificate for this device
    async fn store_device_certificate(&self, device_cert: &crate::crypto::DeviceCert) -> Result<()>;

    /// Get device certificate for this device
    async fn get_device_certificate(&self) -> Result<Option<crate::crypto::DeviceCert>>;

    /// Store the device key for this device
    async fn store_device_key(&self, device_key: &crate::crypto::DeviceKey) -> Result<()>;

    /// Get the device key for this device
    async fn get_device_key(&self) -> Result<crate::crypto::DeviceKey>;

    /// Get storage statistics
    async fn get_stats(&self) -> Result<StorageStats>;

    /// Store attachment metadata and its chunk mapping
    async fn store_attachment(&self, message_id: &MessageId, filename: &str, size: u64, file_hash: &[u8; 32], mime_type: Option<String>, chunk_ids: &Vec<[u8; 32]>) -> Result<i64>;

    /// Get attachments for a message
    async fn get_attachments_for_message(&self, message_id: &MessageId) -> Result<Vec<Attachment>>;

    /// Get chunk ids for an attachment
    async fn get_attachment_chunks(&self, attachment_id: i64) -> Result<Vec<[u8; 32]>>;

    /// Get all attachments (including deleted ones for testing)
    async fn get_all_attachments(&self) -> Result<Vec<Attachment>>;

    /// Get attachment by ID
    async fn get_attachment_by_id(&self, attachment_id: i64) -> Result<Option<Attachment>>;

    /// Get attachments by file hash (for deduplication)
    async fn get_attachments_by_file_hash(&self, file_hash: &[u8; 32]) -> Result<Vec<Attachment>>;

    /// Delete an attachment (soft delete)
    async fn delete_attachment(&self, attachment_id: i64) -> Result<()>;

    /// Purge an attachment (hard delete)
    async fn purge_attachment(&self, attachment_id: i64) -> Result<()>;

    /// Update attachment access time
    async fn update_attachment_access_time(&self, attachment_id: i64) -> Result<()>;

    /// Get attachment statistics
    async fn get_attachment_stats(&self) -> Result<AttachmentStats>;

    /// Garbage collect orphaned chunks and attachments
    async fn garbage_collect_attachments(&self) -> Result<GarbageCollectionStats>;
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub operation_count: usize,
    pub message_count: usize,
    pub chunk_count: usize,
    pub total_size: u64,
}

/// Attachment statistics
#[derive(Debug, Clone)]
pub struct AttachmentStats {
    pub total_attachments: usize,
    pub active_attachments: usize,
    pub deleted_attachments: usize,
    pub purged_attachments: usize,
    pub total_size: u64,
    pub unique_files: usize, // Files with unique hashes (deduplication count)
}

/// Garbage collection statistics
#[derive(Debug, Clone)]
pub struct GarbageCollectionStats {
    pub chunks_removed: usize,
    pub attachments_removed: usize,
    pub space_freed: u64,
}

/// Attachment metadata
#[derive(Debug, Clone)]
pub struct Attachment {
    pub id: i64,
    pub message_id: MessageId,
    pub filename: String,
    pub size: u64,
    pub file_hash: [u8; 32], // BLAKE3 hash of entire file for deduplication
    pub mime_type: Option<String>, // MIME type detection
    pub status: AttachmentStatus, // Active, deleted, purged
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: Option<chrono::DateTime<chrono::Utc>>, // For access tracking
}

/// Attachment status for lifecycle management
#[derive(Debug, Clone, PartialEq)]
pub enum AttachmentStatus {
    Active,   // Normal attachment
    Deleted,  // Soft deleted
    Purged,   // Hard deleted, chunks may be garbage collected
}
