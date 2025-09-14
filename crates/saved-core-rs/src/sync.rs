//! Sync protocol implementation for SAVED
//!
//! Handles synchronization of operations and chunks between devices
//! using the CRDT event log and content-addressed storage.

use crate::error::Result;
use crate::events::{Op, OpHash, EventLog, Operation};
use crate::storage::{Storage, ChunkId};
use crate::crypto::{VaultKey, DeviceKey, blake3_hash};
use crate::types::{Event, MessageId};
use std::path::PathBuf;
use std::sync::mpsc;

/// Sync manager for coordinating synchronization between devices
pub struct SyncManager {
    /// Storage backend
    storage: Storage,
    /// Event log
    event_log: EventLog,
    /// Vault key for encryption
    vault_key: VaultKey,
    /// Device key for signing
    device_key: DeviceKey,
    /// Event sender for notifications
    event_sender: mpsc::Sender<Event>,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(
        storage: Storage,
        vault_key: VaultKey,
        device_key: DeviceKey,
        event_sender: mpsc::Sender<Event>,
    ) -> Self {
        let event_log = EventLog::new();
        
        Self {
            storage,
            event_log,
            vault_key,
            device_key,
            event_sender,
        }
    }

    /// Create a new message
    pub async fn create_message(&mut self, body: String, attachments: Vec<PathBuf>) -> Result<MessageId> {
        let msg_id = MessageId::new();
        
        // Process attachments
        let mut attachment_cids = Vec::new();
        for attachment_path in attachments {
            let chunk_cids = self.process_attachment(&attachment_path).await?;
            attachment_cids.extend(chunk_cids);
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
            self.storage.store_chunk(chunk_id, &encrypted_chunk)?;
            
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
        let op = Op::new(op_id, self.event_log.current_lamport() + 1, parents, operation);
        let op_hash = op.hash();
        
        // Add to event log
        self.event_log.add_operation(op.clone())?;
        
        // Store in database
        self.storage.store_operation(&op)?;
        
        Ok(op_hash)
    }
}