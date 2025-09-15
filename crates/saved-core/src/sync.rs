//! Sync protocol implementation for SAVED
//!
//! Handles synchronization of operations and chunks between devices
//! using the CRDT event log and content-addressed storage.

use crate::crypto::{blake3_hash, DeviceKey, VaultKey};
use crate::error::Result;
use crate::events::{EventLog, Op, OpHash, Operation, OpId};
use crate::storage::{sqlite::ChunkId, Storage};
use crate::types::MessageId;
use std::path::PathBuf;
// Collections imported for future use
use chrono::{DateTime, Utc};

/// Sync manager for coordinating synchronization between devices
pub struct SyncManager {
    /// Storage backend
    storage: Box<dyn Storage>,
    /// Storage path (for testing)
    pub storage_path: PathBuf,
    /// Event log
    event_log: EventLog,
    /// Vault key for encryption
    vault_key: VaultKey,
    /// Device key for signing
    device_key: DeviceKey,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(
        storage: Box<dyn Storage>,
        storage_path: PathBuf,
        vault_key: VaultKey,
        device_key: DeviceKey,
    ) -> Self {
        let event_log = EventLog::new();

        Self {
            storage,
            storage_path,
            event_log,
            vault_key,
            device_key,
        }
    }

    /// Initialize from persisted storage (load operations into the event log)
    pub async fn initialize(&mut self) -> Result<()> {
        let ops = self.storage.get_all_operations().await?;
        for op in ops {
            // Rebuild event log state by re-adding operations
            // Ignore errors for duplicates
            let _ = self.event_log.add_operation(op);
        }
        Ok(())
    }

    /// Create a new message
    pub async fn create_message(
        &mut self,
        body: String,
        attachments: Vec<PathBuf>,
    ) -> Result<MessageId> {
        let msg_id = MessageId::new();

        // Process attachments
        let mut attachment_cids = Vec::new();
        for attachment_path in attachments {
            let chunk_cids = self.process_attachment(&attachment_path).await?;
            attachment_cids.extend(chunk_cids);
            // Store attachment metadata
            let filename = attachment_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let size = tokio::fs::metadata(&attachment_path).await.map(|m| m.len()).unwrap_or(0);
            let _ = self.storage.store_attachment(&msg_id, filename, size, &attachment_cids).await;
        }

        // Create operation
        let operation = Operation::CreateMessage {
            msg_id: msg_id.0,
            feed_id: "default".to_string(),
            body,
            attachments: attachment_cids,
            created_at: chrono::Utc::now(),
        };

        // Add to event log
        self.add_operation(operation).await?;

        Ok(msg_id)
    }

    /// Edit an existing message
    pub async fn edit_message(&mut self, msg_id: MessageId, new_body: String) -> Result<()> {
        let operation = Operation::EditMessage {
            msg_id: msg_id.0,
            body: new_body,
            edited_at: chrono::Utc::now(),
        };

        self.add_operation(operation).await?;
        Ok(())
    }

    /// Delete a message (soft delete)
    pub async fn delete_message(&mut self, msg_id: MessageId) -> Result<()> {
        let operation = Operation::DeleteMessage {
            msg_id: msg_id.0,
            reason: None,
            deleted_at: chrono::Utc::now(),
        };

        self.add_operation(operation).await?;
        Ok(())
    }

    /// Purge a message (hard delete)
    pub async fn purge_message(&mut self, msg_id: MessageId) -> Result<()> {
        let operation = Operation::Purge {
            msg_id: msg_id.0,
            timestamp: chrono::Utc::now(),
        };

        self.add_operation(operation).await?;
        Ok(())
    }

    /// Process an attachment file
    async fn process_attachment(&mut self, path: &PathBuf) -> Result<Vec<ChunkId>> {
        let file_data = tokio::fs::read(path).await?;
        let mut chunk_cids = Vec::new();

        // Chunk the file (2 MiB chunks)
        const CHUNK_SIZE: usize = 2 * 1024 * 1024;
        let mut offset = 0;

        while offset < file_data.len() {
            let end = std::cmp::min(offset + CHUNK_SIZE, file_data.len());
            let chunk_data = &file_data[offset..end];

            // Compute chunk ID (BLAKE3 hash of plaintext)
            let chunk_id = blake3_hash(chunk_data);

            // Encrypt chunk with convergent encryption
            let chunk_key = crate::crypto::derive_chunk_key(&self.vault_key, &chunk_id)?;
            let nonce = crate::crypto::generate_nonce();
            let encrypted_chunk = crate::crypto::encrypt(&chunk_key, &nonce, chunk_data)?;

            // Store chunk
            self.storage
                .store_chunk(&chunk_id, &encrypted_chunk)
                .await?;

            chunk_cids.push(chunk_id);
            offset = end;
        }

        Ok(chunk_cids)
    }

    /// Add an operation to the event log
    async fn add_operation(&mut self, operation: Operation) -> Result<OpHash> {
        // Create operation
        let op_id = crate::events::OpId::new(
            self.device_key.public_key_bytes(),
            self.event_log.current_lamport() + 1,
        );

        let parents = self.event_log.get_heads().iter().cloned().collect();
        let op = Op::new(
            op_id,
            self.event_log.current_lamport() + 1,
            parents,
            operation.clone(),
        );
        let op_hash = op.hash();

        // Add to event log
        self.event_log.add_operation(op.clone())?;

        // Store in database
        self.storage.store_operation(&op).await?;

        // Also create and store Message objects for testing
        match operation {
            Operation::CreateMessage {
                msg_id,
                body,
                created_at,
                ..
            } => {
                let message = crate::types::Message {
                    id: crate::types::MessageId(msg_id),
                    content: body,
                    created_at,
                    is_deleted: false,
                    is_purged: false,
                };
                self.storage.store_message(&message).await?;
            }
            Operation::EditMessage { msg_id, body, .. } => {
                // Update existing message
                if let Some(mut message) = self
                    .storage
                    .get_message(&crate::types::MessageId(msg_id))
                    .await?
                {
                    message.content = body;
                    self.storage.store_message(&message).await?;
                }
            }
            Operation::DeleteMessage { msg_id, .. } => {
                // Mark message as deleted
                if let Some(mut message) = self
                    .storage
                    .get_message(&crate::types::MessageId(msg_id))
                    .await?
                {
                    message.is_deleted = true;
                    self.storage.store_message(&message).await?;
                }
            }
            Operation::Purge { msg_id, .. } => {
                // Remove message completely
                self.storage
                    .delete_message(&crate::types::MessageId(msg_id))
                    .await?;
            }
            _ => {
                // Other operations don't need message handling
            }
        }

        Ok(op_hash)
    }

    /// Get pending operations for synchronization
    pub async fn get_pending_operations(&self) -> Result<Vec<Op>> {
        // Return all operations for now - in a real implementation this would track
        // which operations have been sent to which peers
        self.storage.get_all_operations().await
    }

    /// Create an attachment operation (for testing)
    pub async fn create_attachment_operation(
        &self,
        _msg_id: MessageId,
        _filename: String,
        _content: Vec<u8>,
    ) -> Result<Op> {
        // For testing purposes, create a dummy operation
        let operation = Operation::Attach {
            msg_id: [0u8; 32],
            attachment_cids: Vec::new(),
        };

        let op_id = crate::events::OpId::new(
            self.device_key.public_key_bytes(),
            self.event_log.current_lamport() + 1,
        );

        let parents = self.event_log.get_heads().iter().cloned().collect();
        let op = Op::new(
            op_id,
            self.event_log.current_lamport() + 1,
            parents,
            operation,
        );

        Ok(op)
    }

    /// Get all messages from storage (for testing)
    pub async fn get_all_messages(&self) -> Result<Vec<crate::types::Message>> {
        self.storage.get_all_messages().await
    }

    /// Get storage reference for account key operations
    pub fn storage(&self) -> &dyn Storage {
        self.storage.as_ref()
    }

    /// Get mutable storage reference for account key operations
    pub fn storage_mut(&mut self) -> &mut dyn Storage {
        self.storage.as_mut()
    }

    /// Get event log reference
    pub fn event_log(&self) -> &EventLog {
        &self.event_log
    }

    /// Apply an operation from another device with CRDT conflict resolution
    pub async fn apply_operation(&mut self, op: Op) -> Result<()> {
        let op_hash = op.hash();
        
        // Check if we already have this operation
        if self.event_log.get_operation(&op_hash).is_some() {
            return Ok(()); // Already applied
        }

        // Verify the operation is valid
        self.verify_operation(&op)?;

        // Add to event log
        self.event_log.add_operation(op.clone())?;

        // Store in database
        self.storage.store_operation(&op).await?;

        // Apply CRDT conflict resolution
        self.resolve_conflicts_for_message(&op).await?;

        Ok(())
    }

    /// Verify an operation is valid
    fn verify_operation(&self, op: &Op) -> Result<()> {
        // Check that all parent operations exist
        for parent_hash in &op.parents {
            if self.event_log.get_operation(parent_hash).is_none() {
                return Err(crate::error::Error::Sync(
                    format!("Missing parent operation: {}", hex::encode(parent_hash))
                ));
            }
        }

        // Check lamport timestamp is reasonable
        if op.lamport > self.event_log.current_lamport() + 1000 {
            return Err(crate::error::Error::Sync(
                "Lamport timestamp too far in future".to_string()
            ));
        }

        Ok(())
    }

    /// Resolve conflicts for a message using CRDT semantics
    async fn resolve_conflicts_for_message(&mut self, op: &Op) -> Result<()> {
        let msg_id = match &op.operation {
            Operation::CreateMessage { msg_id, .. } => *msg_id,
            Operation::EditMessage { msg_id, .. } => *msg_id,
            Operation::DeleteMessage { msg_id, .. } => *msg_id,
            Operation::Purge { msg_id, .. } => *msg_id,
            _ => return Ok(()), // Not a message operation
        };

        // Get all operations for this message
        let message_ops = self.get_operations_for_message(msg_id);
        
        // Apply CRDT conflict resolution
        let resolved_state = self.crdt_resolve_message_conflicts(&message_ops)?;
        
        // Update the materialized view
        self.update_message_materialized_view(msg_id, &resolved_state).await?;

        Ok(())
    }

    /// Get all operations for a specific message from the event log
    fn get_operations_for_message(&self, msg_id: [u8; 32]) -> Vec<Op> {
        self.event_log.get_all_operations()
            .into_iter()
            .filter(|op| {
                matches!(
                    &op.operation,
                    Operation::CreateMessage { msg_id: id, .. } |
                    Operation::EditMessage { msg_id: id, .. } |
                    Operation::DeleteMessage { msg_id: id, .. } |
                    Operation::Purge { msg_id: id, .. }
                    if *id == msg_id
                )
            })
            .cloned()
            .collect()
    }

    /// CRDT conflict resolution for message operations
    fn crdt_resolve_message_conflicts(&self, operations: &[Op]) -> Result<ResolvedMessageState> {
        if operations.is_empty() {
            return Ok(ResolvedMessageState::None);
        }

        // Sort operations by lamport timestamp, then by device ID for deterministic ordering
        let mut sorted_ops = operations.to_vec();
        sorted_ops.sort_by(|a, b| {
            a.lamport.cmp(&b.lamport)
                .then_with(|| a.id.device_pubkey.cmp(&b.id.device_pubkey))
        });

        let mut state = ResolvedMessageState::None;
        let mut last_edit: Option<Op> = None;
        let mut is_deleted = false;
        let mut is_purged = false;

        for op in sorted_ops {
            match &op.operation {
                Operation::CreateMessage { body, created_at, .. } => {
                    if state == ResolvedMessageState::None {
                        state = ResolvedMessageState::Created {
                            body: body.clone(),
                            created_at: *created_at,
                        };
                        last_edit = Some(op.clone());
                    }
                }
                Operation::EditMessage { body, edited_at, .. } => {
                    if !is_deleted && !is_purged {
                        // Only apply edits if message isn't deleted/purged
                        // Use last-write-wins with lamport timestamp tiebreaker
                        if last_edit.is_none() || 
                           op.lamport > last_edit.as_ref().unwrap().lamport ||
                           (op.lamport == last_edit.as_ref().unwrap().lamport && 
                            op.id.device_pubkey > last_edit.as_ref().unwrap().id.device_pubkey) {
                            state = ResolvedMessageState::Edited {
                                body: body.clone(),
                                last_edited_at: *edited_at,
                            };
                            last_edit = Some(op.clone());
                        }
                    }
                }
                Operation::DeleteMessage { deleted_at, .. } => {
                    if !is_purged {
                        is_deleted = true;
                        state = ResolvedMessageState::Deleted {
                            deleted_at: *deleted_at,
                        };
                    }
                }
                Operation::Purge { timestamp, .. } => {
                    is_purged = true;
                    state = ResolvedMessageState::Purged {
                        purged_at: *timestamp,
                    };
                }
                _ => {} // Other operations don't affect message state
            }
        }

        Ok(state)
    }

    /// Update the materialized view of a message based on CRDT resolution
    async fn update_message_materialized_view(
        &mut self,
        msg_id: [u8; 32],
        state: &ResolvedMessageState,
    ) -> Result<()> {
        let message_id = MessageId(msg_id);

        match state {
            ResolvedMessageState::None => {
                // Message doesn't exist, ensure it's not in storage
                self.storage.delete_message(&message_id).await?;
            }
            ResolvedMessageState::Created { body, created_at } => {
                let message = crate::types::Message {
                    id: message_id,
                    content: body.clone(),
                    created_at: *created_at,
                    is_deleted: false,
                    is_purged: false,
                };
                self.storage.store_message(&message).await?;
            }
            ResolvedMessageState::Edited { body, last_edited_at } => {
                let message = crate::types::Message {
                    id: message_id,
                    content: body.clone(),
                    created_at: *last_edited_at, // Use last edit time as created time for simplicity
                    is_deleted: false,
                    is_purged: false,
                };
                self.storage.store_message(&message).await?;
            }
            ResolvedMessageState::Deleted { deleted_at } => {
                // Mark as deleted but keep in storage
                if let Some(mut message) = self.storage.get_message(&message_id).await? {
                    message.is_deleted = true;
                    self.storage.store_message(&message).await?;
                } else {
                    // If message doesn't exist, create a deleted placeholder
                    let message = crate::types::Message {
                        id: message_id,
                        content: "".to_string(), // Empty content for deleted message
                        created_at: *deleted_at,
                        is_deleted: true,
                        is_purged: false,
                    };
                    self.storage.store_message(&message).await?;
                }
            }
            ResolvedMessageState::Purged { .. } => {
                // Remove completely from storage
                self.storage.delete_message(&message_id).await?;
            }
        }

        Ok(())
    }

    /// Get operations that need to be synchronized to another device
    pub async fn get_operations_for_sync(&self, since_heads: &[OpHash]) -> Result<Vec<Op>> {
        let operations = self.event_log.get_operations_since(since_heads);
        Ok(operations.into_iter().cloned().collect())
    }

    /// Get heads for synchronization
    pub fn get_sync_heads(&self) -> Vec<OpHash> {
        self.event_log.get_heads().iter().cloned().collect()
    }

    /// Check if we have all operations up to a certain point
    pub fn has_operations_up_to(&self, up_to: &OpId) -> bool {
        self.event_log.has_operations_up_to(up_to)
    }
}

/// Resolved state of a message after CRDT conflict resolution
#[derive(Debug, Clone, PartialEq)]
enum ResolvedMessageState {
    None,
    Created {
        body: String,
        created_at: DateTime<Utc>,
    },
    Edited {
        body: String,
        last_edited_at: DateTime<Utc>,
    },
    Deleted {
        deleted_at: DateTime<Utc>,
    },
    Purged {
        purged_at: DateTime<Utc>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{generate_vault_key, DeviceKey};
    use crate::storage::memory::MemoryStorage;
    // StorageBackend not used in tests
    use tempfile::TempDir;

    /// Create a test sync manager with in-memory storage
    async fn create_test_sync_manager() -> SyncManager {
        let vault_key = generate_vault_key();
        let device_key = DeviceKey::generate();
        let storage = Box::new(MemoryStorage::new());
        let temp_dir = TempDir::new().unwrap();
        
        let mut sync_manager = SyncManager::new(
            storage,
            temp_dir.path().to_path_buf(),
            vault_key,
            device_key,
        );
        sync_manager.initialize().await.unwrap();
        sync_manager
    }

    /// Create a test operation
    fn create_test_operation(
        device_key: &DeviceKey,
        lamport: u64,
        parents: Vec<OpHash>,
        operation: Operation,
    ) -> Op {
        let op_id = OpId::new(device_key.public_key_bytes(), 1);
        Op::new(op_id, lamport, parents, operation)
    }

    #[tokio::test]
    async fn test_crdt_create_message() {
        let mut sync_manager = create_test_sync_manager().await;
        let device_key = DeviceKey::generate();
        
        // Create a message
        let msg_id = [1u8; 32];
        let operation = Operation::CreateMessage {
            msg_id,
            feed_id: "default".to_string(),
            body: "Hello, SAVED!".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        };
        
        let op = create_test_operation(&device_key, 1, Vec::new(), operation);
        sync_manager.apply_operation(op).await.unwrap();
        
        // Check that message was created
        let messages = sync_manager.get_all_messages().await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Hello, SAVED!");
        assert!(!messages[0].is_deleted);
    }

    #[tokio::test]
    async fn test_crdt_edit_message() {
        let mut sync_manager = create_test_sync_manager().await;
        let device_key = DeviceKey::generate();
        let msg_id = [1u8; 32];
        
        // Create a message
        let create_op = create_test_operation(
            &device_key,
            1,
            Vec::new(),
            Operation::CreateMessage {
                msg_id,
                feed_id: "default".to_string(),
                body: "Original content".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        );
        let create_op_hash = create_op.hash();
        sync_manager.apply_operation(create_op).await.unwrap();
        
        // Edit the message
        let edit_op = create_test_operation(
            &device_key,
            2,
            vec![create_op_hash],
            Operation::EditMessage {
                msg_id,
                body: "Edited content".to_string(),
                edited_at: Utc::now(),
            },
        );
        sync_manager.apply_operation(edit_op).await.unwrap();
        
        // Check that message was edited
        let messages = sync_manager.get_all_messages().await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Edited content");
    }

    #[tokio::test]
    async fn test_crdt_delete_message() {
        let mut sync_manager = create_test_sync_manager().await;
        let device_key = DeviceKey::generate();
        let msg_id = [1u8; 32];
        
        // Create a message
        let create_op = create_test_operation(
            &device_key,
            1,
            Vec::new(),
            Operation::CreateMessage {
                msg_id,
                feed_id: "default".to_string(),
                body: "To be deleted".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        );
        let create_op_hash = create_op.hash();
        sync_manager.apply_operation(create_op).await.unwrap();
        
        // Check that message was created
        let messages_after_create = sync_manager.get_all_messages().await.unwrap();
        
        // Delete the message
        let delete_op = create_test_operation(
            &device_key,
            2,
            vec![create_op_hash],
            Operation::DeleteMessage {
                msg_id,
                reason: Some("Test deletion".to_string()),
                deleted_at: Utc::now(),
            },
        );
        sync_manager.apply_operation(delete_op).await.unwrap();
        
        // Check that message is marked as deleted
        let messages = sync_manager.storage.get_all_messages_including_deleted().await.unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].is_deleted);
    }

    #[tokio::test]
    async fn test_crdt_conflict_resolution_last_write_wins() {
        let mut sync_manager = create_test_sync_manager().await;
        let device_key1 = DeviceKey::generate();
        let device_key2 = DeviceKey::generate();
        let msg_id = [1u8; 32];
        
        // Create a message
        let create_op = create_test_operation(
            &device_key1,
            1,
            Vec::new(),
            Operation::CreateMessage {
                msg_id,
                feed_id: "default".to_string(),
                body: "Original content".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        );
        let create_op_hash = create_op.hash();
        sync_manager.apply_operation(create_op).await.unwrap();
        
        // Two concurrent edits with same lamport timestamp
        let edit1_op = create_test_operation(
            &device_key1,
            2,
            vec![create_op_hash],
            Operation::EditMessage {
                msg_id,
                body: "Edit from device 1".to_string(),
                edited_at: Utc::now(),
            },
        );
        
        let edit2_op = create_test_operation(
            &device_key2,
            2, // Same lamport timestamp
            vec![create_op_hash],
            Operation::EditMessage {
                msg_id,
                body: "Edit from device 2".to_string(),
                edited_at: Utc::now(),
            },
        );
        
        // Apply both edits
        sync_manager.apply_operation(edit1_op).await.unwrap();
        sync_manager.apply_operation(edit2_op).await.unwrap();
        
        // Check that the edit with higher device ID wins (deterministic tiebreaker)
        let messages = sync_manager.get_all_messages().await.unwrap();
        assert_eq!(messages.len(), 1);
        
        // The winner should be the one with higher device_pubkey
        if device_key2.public_key_bytes() > device_key1.public_key_bytes() {
            assert_eq!(messages[0].content, "Edit from device 2");
        } else {
            assert_eq!(messages[0].content, "Edit from device 1");
        }
    }

    #[tokio::test]
    async fn test_crdt_delete_after_edit() {
        let mut sync_manager = create_test_sync_manager().await;
        let device_key = DeviceKey::generate();
        let msg_id = [1u8; 32];
        
        // Create a message
        let create_op = create_test_operation(
            &device_key,
            1,
            Vec::new(),
            Operation::CreateMessage {
                msg_id,
                feed_id: "default".to_string(),
                body: "Original content".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        );
        let create_op_hash = create_op.hash();
        sync_manager.apply_operation(create_op).await.unwrap();
        
        // Edit the message
        let edit_op = create_test_operation(
            &device_key,
            2,
            vec![create_op_hash],
            Operation::EditMessage {
                msg_id,
                body: "Edited content".to_string(),
                edited_at: Utc::now(),
            },
        );
        sync_manager.apply_operation(edit_op.clone()).await.unwrap();
        
        // Delete the message (should override the edit)
        let edit_op_hash = edit_op.hash();
        let delete_op = create_test_operation(
            &device_key,
            3,
            vec![edit_op_hash],
            Operation::DeleteMessage {
                msg_id,
                reason: None,
                deleted_at: Utc::now(),
            },
        );
        sync_manager.apply_operation(delete_op).await.unwrap();
        
        // Check that message is deleted (not edited)
        let messages = sync_manager.storage.get_all_messages_including_deleted().await.unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].is_deleted);
    }

    #[tokio::test]
    async fn test_crdt_purge_overrides_all() {
        let mut sync_manager = create_test_sync_manager().await;
        let device_key = DeviceKey::generate();
        let msg_id = [1u8; 32];
        
        // Create a message
        let create_op = create_test_operation(
            &device_key,
            1,
            Vec::new(),
            Operation::CreateMessage {
                msg_id,
                feed_id: "default".to_string(),
                body: "Original content".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        );
        let create_op_hash = create_op.hash();
        sync_manager.apply_operation(create_op).await.unwrap();
        
        // Edit the message
        let edit_op = create_test_operation(
            &device_key,
            2,
            vec![create_op_hash],
            Operation::EditMessage {
                msg_id,
                body: "Edited content".to_string(),
                edited_at: Utc::now(),
            },
        );
        sync_manager.apply_operation(edit_op.clone()).await.unwrap();
        
        // Delete the message
        let edit_op_hash = edit_op.hash();
        let delete_op = create_test_operation(
            &device_key,
            3,
            vec![edit_op_hash],
            Operation::DeleteMessage {
                msg_id,
                reason: None,
                deleted_at: Utc::now(),
            },
        );
        sync_manager.apply_operation(delete_op.clone()).await.unwrap();
        
        // Purge the message (should remove it completely)
        let delete_op_hash = delete_op.hash();
        let purge_op = create_test_operation(
            &device_key,
            4,
            vec![delete_op_hash],
            Operation::Purge {
                msg_id,
                timestamp: Utc::now(),
            },
        );
        sync_manager.apply_operation(purge_op).await.unwrap();
        
        // Check that message is completely removed
        let messages = sync_manager.storage.get_all_messages_including_deleted().await.unwrap();
        assert_eq!(messages.len(), 0);
    }

    #[tokio::test]
    async fn test_sync_heads_and_operations() {
        let mut sync_manager = create_test_sync_manager().await;
        let device_key = DeviceKey::generate();
        
        // Initially no heads
        let heads = sync_manager.get_sync_heads();
        assert_eq!(heads.len(), 0);
        
        // Create a message
        let msg_id = [1u8; 32];
        let create_op = create_test_operation(
            &device_key,
            1,
            Vec::new(),
            Operation::CreateMessage {
                msg_id,
                feed_id: "default".to_string(),
                body: "Test message".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        );
        sync_manager.apply_operation(create_op).await.unwrap();
        
        // Should have one head now
        let heads = sync_manager.get_sync_heads();
        assert_eq!(heads.len(), 1);
        
        // Get operations since empty heads (should return all operations)
        let ops = sync_manager.get_operations_for_sync(&[]).await.unwrap();
        assert_eq!(ops.len(), 1);
    }

    #[tokio::test]
    async fn test_operation_verification() {
        let mut sync_manager = create_test_sync_manager().await;
        let device_key = DeviceKey::generate();
        let msg_id = [1u8; 32];
        
        // Try to apply an operation with a non-existent parent
        let fake_parent = [0u8; 32];
        let invalid_op = create_test_operation(
            &device_key,
            1,
            vec![fake_parent],
            Operation::CreateMessage {
                msg_id,
                feed_id: "default".to_string(),
                body: "Invalid operation".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        );
        
        // Should fail verification
        let result = sync_manager.apply_operation(invalid_op).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing parent operation"));
    }
}
