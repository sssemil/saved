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

    /// Get the vault key for encryption
    async fn get_vault_key(&self) -> Result<VaultKey> {
        Ok(self.vault_key)
    }

    /// Get current head operations for synchronization
    pub async fn get_head_operations(&self) -> Vec<OpHash> {
        // Get all operations from storage
        if let Ok(operations) = self.storage.get_all_encrypted_operations().await {
            // Find operations that are not dependencies of other operations
            let mut heads = Vec::new();
            let mut all_ops: std::collections::HashSet<OpHash> = std::collections::HashSet::new();
            
            // Collect all operation hashes
            for op_envelope in &operations {
                if let Some(header) = &op_envelope.header {
                    // TODO: Use proper hash from header when available
                    // For now, use a placeholder hash
                    let hash = blake3_hash(&header.op_id);
                    all_ops.insert(hash);
                }
            }
            
            // Find operations that are not dependencies of other operations
            for op_envelope in &operations {
                if let Some(header) = &op_envelope.header {
                    let is_head = true;
                    // TODO: Check dependencies when available
                    // for dep in &header.dependencies {
                    //     if all_ops.contains(dep) {
                    //         is_head = false;
                    //         break;
                    //     }
                    // }
                    if is_head {
                        let hash = blake3_hash(&header.op_id);
                        heads.push(hash);
                    }
                }
            }
            
            heads
        } else {
            Vec::new()
        }
    }

    /// Apply operations received from peers
    pub async fn apply_peer_operations(&mut self, operations: Vec<crate::protobuf::OpEnvelope>) -> Result<()> {
        for op_envelope in operations {
            // Check if we already have this operation
            if let Some(header) = &op_envelope.header {
                // TODO: Implement has_operation method in storage trait
                // if self.storage.has_operation(&header.hash).await? {
                //     continue;
                // }
                
                // TODO: Validate the operation signature
                // if let Err(e) = op_envelope.verify_signature() {
                //     println!("Invalid operation signature: {}", e);
                //     continue;
                // }
                
                // TODO: Decrypt and verify the operation
                // if let Ok(op) = op_envelope.decrypt_and_verify(&self.vault_key) {
                //     // Add to event log
                //     self.event_log.add_operation(op.clone());
                //     
                //     // Store the encrypted operation
                //     self.storage.store_encrypted_operation(&op_envelope).await?;
                //     
                //     println!("Applied operation from peer: {:?}", header.hash);
                // } else {
                //     println!("Failed to decrypt operation from peer");
                // }
                
                println!("Received operation from peer: {:?}", header.op_id);
            }
        }
        
        Ok(())
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
            let file_data = tokio::fs::read(&attachment_path).await?;
            let file_hash = blake3_hash(&file_data);
            
            let chunk_cids = self.process_attachment(&attachment_path).await?;
            attachment_cids.extend(&chunk_cids);
            
            // Detect MIME type
            let mime_type = self.detect_mime_type(&attachment_path, &file_data);
            
            // Store attachment metadata
            let filename = attachment_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let size = file_data.len() as u64;
            let _ = self.storage.store_attachment(&msg_id, filename, size, &file_hash, mime_type, &chunk_cids).await;
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

    /// Process an attachment file with deduplication
    async fn process_attachment(&mut self, path: &PathBuf) -> Result<Vec<ChunkId>> {
        let file_data = tokio::fs::read(path).await?;
        
        // Compute file hash for deduplication
        let file_hash = blake3_hash(&file_data);
        
        // Check if file already exists (deduplication)
        let existing_attachments = self.storage.get_attachments_by_file_hash(&file_hash).await?;
        if !existing_attachments.is_empty() {
            // File already exists, get its chunks
            let existing_attachment = &existing_attachments[0];
            let existing_chunks = self.storage.get_attachment_chunks(existing_attachment.id).await?;
            
            // Increment reference counts for existing chunks
            for chunk_id in &existing_chunks {
                self.storage.store_chunk(chunk_id, &[]).await?; // This increments ref count
            }
            
            return Ok(existing_chunks);
        }
        
        // New file, process normally
        let mut chunk_cids = Vec::new();
        const CHUNK_SIZE: usize = 2 * 1024 * 1024;
        let mut offset = 0;

        while offset < file_data.len() {
            let end = std::cmp::min(offset + CHUNK_SIZE, file_data.len());
            let chunk_data = &file_data[offset..end];

            // Compute chunk ID (BLAKE3 hash of plaintext)
            let chunk_id = blake3_hash(chunk_data);

            // Check if chunk already exists
            if !self.storage.has_chunk(&chunk_id).await? {
                // Encrypt chunk with convergent encryption
                let chunk_key = crate::crypto::derive_chunk_key(&self.vault_key, &chunk_id)?;
                let nonce = crate::crypto::generate_nonce();
                let encrypted_chunk = crate::crypto::encrypt(&chunk_key, &nonce, chunk_data)?;

                // Store chunk with nonce prepended
                let mut chunk_with_nonce = Vec::new();
                chunk_with_nonce.extend_from_slice(&nonce);
                chunk_with_nonce.extend_from_slice(&encrypted_chunk);
                self.storage.store_chunk(&chunk_id, &chunk_with_nonce).await?;
            } else {
                // Chunk exists, just increment reference count
                self.storage.store_chunk(&chunk_id, &[]).await?;
            }

            chunk_cids.push(chunk_id);
            offset = end;
        }

        Ok(chunk_cids)
    }

    /// Detect MIME type from file extension and content
    fn detect_mime_type(&self, path: &PathBuf, _data: &[u8]) -> Option<String> {
        // Simple MIME type detection based on file extension
        // In a production system, you might want to use a library like `infer` for content-based detection
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            match extension.to_lowercase().as_str() {
                "txt" => Some("text/plain".to_string()),
                "md" => Some("text/markdown".to_string()),
                "html" | "htm" => Some("text/html".to_string()),
                "css" => Some("text/css".to_string()),
                "js" => Some("application/javascript".to_string()),
                "json" => Some("application/json".to_string()),
                "xml" => Some("application/xml".to_string()),
                "pdf" => Some("application/pdf".to_string()),
                "zip" => Some("application/zip".to_string()),
                "tar" => Some("application/x-tar".to_string()),
                "gz" => Some("application/gzip".to_string()),
                "jpg" | "jpeg" => Some("image/jpeg".to_string()),
                "png" => Some("image/png".to_string()),
                "gif" => Some("image/gif".to_string()),
                "svg" => Some("image/svg+xml".to_string()),
                "mp3" => Some("audio/mpeg".to_string()),
                "wav" => Some("audio/wav".to_string()),
                "mp4" => Some("video/mp4".to_string()),
                "avi" => Some("video/x-msvideo".to_string()),
                "mov" => Some("video/quicktime".to_string()),
                _ => None,
            }
        } else {
            None
        }
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

        // Encrypt the operation for secure storage
        let vault_key = self.get_vault_key().await?;
        let encrypted_envelope = op.encrypt(&vault_key, &self.device_key)?;
        
        // Store encrypted operation in database
        self.storage.store_encrypted_operation(&encrypted_envelope).await?;

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

    /// Download missing chunks for an attachment on-demand
    pub async fn download_attachment_chunks(&mut self, attachment_id: i64, network_manager: Option<&mut crate::networking::NetworkManager>) -> Result<()> {
        // Get attachment metadata
        let attachment = match self.storage.get_attachment_by_id(attachment_id).await? {
            Some(att) => att,
            None => return Err(crate::error::Error::MessageNotFound),
        };

        // Get chunk IDs for this attachment
        let chunk_ids = self.storage.get_attachment_chunks(attachment_id).await?;
        
        // Check which chunks we're missing
        let mut missing_chunks = Vec::new();
        for chunk_id in &chunk_ids {
            if !self.storage.has_chunk(chunk_id).await? {
                missing_chunks.push(*chunk_id);
            }
        }

        if missing_chunks.is_empty() {
            return Ok(()); // All chunks are already available
        }

        println!("Missing {} chunks for attachment '{}', requesting from peers...", 
                missing_chunks.len(), attachment.filename);

        // If we have a network manager, try to download missing chunks
        if let Some(network_manager) = network_manager {
            // Request chunk availability from peers
            if let Err(e) = network_manager.request_chunk_availability(missing_chunks.clone()).await {
                println!("Failed to request chunk availability: {}", e);
                return Err(e);
            }

            // Request the actual chunks
            if let Err(e) = network_manager.request_chunks(missing_chunks).await {
                println!("Failed to request chunks: {}", e);
                return Err(e);
            }
        } else {
            return Err(crate::error::Error::Network("No network manager available for chunk download".to_string()));
        }

        Ok(())
    }

    /// Reconstruct a file from its chunks
    pub async fn reconstruct_attachment_file(&self, attachment_id: i64) -> Result<Vec<u8>> {
        // Get attachment metadata
        let attachment = match self.storage.get_attachment_by_id(attachment_id).await? {
            Some(att) => att,
            None => return Err(crate::error::Error::MessageNotFound),
        };

        // Get chunk IDs for this attachment
        let chunk_ids = self.storage.get_attachment_chunks(attachment_id).await?;
        
        // Reconstruct file from chunks
        let mut file_data = Vec::new();
        for chunk_id in &chunk_ids {
            // Check if we have this chunk
            if !self.storage.has_chunk(chunk_id).await? {
                return Err(crate::error::Error::Storage(format!("Chunk {} not found", hex::encode(chunk_id))));
            }

            // Get the encrypted chunk data
            let encrypted_chunk = match self.storage.get_chunk(chunk_id).await? {
                Some(data) => data,
                None => return Err(crate::error::Error::Storage(format!("Chunk {} data not found", hex::encode(chunk_id)))),
            };

            // Decrypt the chunk
            let chunk_key = crate::crypto::derive_chunk_key(&self.vault_key, chunk_id)?;
            let nonce_slice = &encrypted_chunk[0..24]; // First 24 bytes are nonce
            let mut nonce = [0u8; 24];
            nonce.copy_from_slice(nonce_slice);
            let ciphertext = &encrypted_chunk[24..];
            let decrypted_chunk = crate::crypto::decrypt(&chunk_key, &nonce, ciphertext)?;

            // Append to file data
            file_data.extend_from_slice(&decrypted_chunk);
        }

        // Verify file hash matches
        let computed_hash = blake3_hash(&file_data);
        if computed_hash != attachment.file_hash {
            return Err(crate::error::Error::Crypto("File hash verification failed".to_string()));
        }

        // Update access time
        self.storage.update_attachment_access_time(attachment_id).await?;

        Ok(file_data)
    }

    /// Check if an attachment is fully available (all chunks present)
    pub async fn is_attachment_available(&self, attachment_id: i64) -> Result<bool> {
        let chunk_ids = self.storage.get_attachment_chunks(attachment_id).await?;
        
        for chunk_id in &chunk_ids {
            if !self.storage.has_chunk(chunk_id).await? {
                return Ok(false);
            }
        }
        
        Ok(true)
    }

    /// Get attachment availability status
    pub async fn get_attachment_availability(&self, attachment_id: i64) -> Result<AttachmentAvailability> {
        let attachment = match self.storage.get_attachment_by_id(attachment_id).await? {
            Some(att) => att,
            None => return Err(crate::error::Error::MessageNotFound),
        };

        let chunk_ids = self.storage.get_attachment_chunks(attachment_id).await?;
        let mut available_chunks = 0;
        let mut missing_chunks = Vec::new();

        for chunk_id in &chunk_ids {
            if self.storage.has_chunk(chunk_id).await? {
                available_chunks += 1;
            } else {
                missing_chunks.push(*chunk_id);
            }
        }

        let total_chunks = chunk_ids.len();
        let is_fully_available = missing_chunks.is_empty();

        Ok(AttachmentAvailability {
            attachment_id,
            filename: attachment.filename,
            total_size: attachment.size,
            total_chunks,
            available_chunks,
            missing_chunks,
            is_fully_available,
            progress_percentage: if total_chunks > 0 { (available_chunks as f64 / total_chunks as f64) * 100.0 } else { 0.0 },
        })
    }

}

/// Attachment availability information
#[derive(Debug, Clone)]
pub struct AttachmentAvailability {
    /// Attachment ID
    pub attachment_id: i64,
    /// Filename
    pub filename: String,
    /// Total file size in bytes
    pub total_size: u64,
    /// Total number of chunks
    pub total_chunks: usize,
    /// Number of available chunks
    pub available_chunks: usize,
    /// List of missing chunk IDs
    pub missing_chunks: Vec<[u8; 32]>,
    /// Whether the attachment is fully available
    pub is_fully_available: bool,
    /// Download progress percentage (0.0 to 100.0)
    pub progress_percentage: f64,
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

    #[tokio::test]
    async fn test_file_deduplication() {
        let storage = Box::new(crate::storage::MemoryStorage::new());
        let vault_key = crate::crypto::generate_vault_key();
        let device_key = crate::crypto::DeviceKey::generate();
        let mut sync_manager = SyncManager::new(storage, PathBuf::from("/tmp"), vault_key, device_key);
        sync_manager.initialize().await.unwrap();

        // Create a test file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello, World!").unwrap();

        // Process the same file twice
        let chunk_cids1 = sync_manager.process_attachment(&file_path).await.unwrap();
        let chunk_cids2 = sync_manager.process_attachment(&file_path).await.unwrap();

        // Should return the same chunk IDs (deduplication)
        assert_eq!(chunk_cids1, chunk_cids2);
        assert_eq!(chunk_cids1.len(), 1); // Small file should be one chunk

        // Check that chunks are properly stored
        for chunk_id in &chunk_cids1 {
            assert!(sync_manager.storage.has_chunk(chunk_id).await.unwrap());
        }
    }

    #[tokio::test]
    async fn test_attachment_metadata_management() {
        let storage = Box::new(crate::storage::MemoryStorage::new());
        let vault_key = crate::crypto::generate_vault_key();
        let device_key = crate::crypto::DeviceKey::generate();
        let mut sync_manager = SyncManager::new(storage, PathBuf::from("/tmp"), vault_key, device_key);
        sync_manager.initialize().await.unwrap();

        // Create a test file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello, World!").unwrap();

        // Create message with attachment
        let msg_id = sync_manager.create_message("Test message".to_string(), vec![file_path]).await.unwrap();

        // Get attachments for the message
        let attachments = sync_manager.storage.get_attachments_for_message(&msg_id).await.unwrap();
        assert_eq!(attachments.len(), 1);

        let attachment = &attachments[0];
        assert_eq!(attachment.filename, "test.txt");
        assert_eq!(attachment.size, 13); // "Hello, World!" length
        assert_eq!(attachment.status, crate::storage::trait_impl::AttachmentStatus::Active);
        assert!(attachment.mime_type.is_some());
        assert_eq!(attachment.mime_type.as_ref().unwrap(), "text/plain");

        // Test attachment statistics
        let stats = sync_manager.storage.get_attachment_stats().await.unwrap();
        assert_eq!(stats.total_attachments, 1);
        assert_eq!(stats.active_attachments, 1);
        assert_eq!(stats.unique_files, 1);
        assert_eq!(stats.total_size, 13);
    }

    #[tokio::test]
    async fn test_attachment_lifecycle() {
        let storage = Box::new(crate::storage::MemoryStorage::new());
        let vault_key = crate::crypto::generate_vault_key();
        let device_key = crate::crypto::DeviceKey::generate();
        let mut sync_manager = SyncManager::new(storage, PathBuf::from("/tmp"), vault_key, device_key);
        sync_manager.initialize().await.unwrap();

        // Create a test file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello, World!").unwrap();

        // Create message with attachment
        let msg_id = sync_manager.create_message("Test message".to_string(), vec![file_path]).await.unwrap();

        // Get attachment ID
        let attachments = sync_manager.storage.get_attachments_for_message(&msg_id).await.unwrap();
        let attachment_id = attachments[0].id;

        // Test soft delete
        sync_manager.storage.delete_attachment(attachment_id).await.unwrap();
        let attachment = sync_manager.storage.get_attachment_by_id(attachment_id).await.unwrap().unwrap();
        assert_eq!(attachment.status, crate::storage::trait_impl::AttachmentStatus::Deleted);

        // Test purge
        sync_manager.storage.purge_attachment(attachment_id).await.unwrap();
        let attachment = sync_manager.storage.get_attachment_by_id(attachment_id).await.unwrap().unwrap();
        assert_eq!(attachment.status, crate::storage::trait_impl::AttachmentStatus::Purged);

        // Test garbage collection
        let gc_stats = sync_manager.storage.garbage_collect_attachments().await.unwrap();
        assert_eq!(gc_stats.attachments_removed, 1);
        assert!(gc_stats.space_freed > 0);
    }

    #[tokio::test]
    async fn test_attachment_access_tracking() {
        let storage = Box::new(crate::storage::MemoryStorage::new());
        let vault_key = crate::crypto::generate_vault_key();
        let device_key = crate::crypto::DeviceKey::generate();
        let mut sync_manager = SyncManager::new(storage, PathBuf::from("/tmp"), vault_key, device_key);
        sync_manager.initialize().await.unwrap();

        // Create a test file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello, World!").unwrap();

        // Create message with attachment
        let msg_id = sync_manager.create_message("Test message".to_string(), vec![file_path]).await.unwrap();

        // Get attachment ID
        let attachments = sync_manager.storage.get_attachments_for_message(&msg_id).await.unwrap();
        let attachment_id = attachments[0].id;

        // Initially no access time
        let attachment = sync_manager.storage.get_attachment_by_id(attachment_id).await.unwrap().unwrap();
        assert!(attachment.last_accessed.is_none());

        // Update access time
        sync_manager.storage.update_attachment_access_time(attachment_id).await.unwrap();

        // Check access time was updated
        let attachment = sync_manager.storage.get_attachment_by_id(attachment_id).await.unwrap().unwrap();
        assert!(attachment.last_accessed.is_some());
    }

    #[tokio::test]
    async fn test_attachment_file_hash_deduplication() {
        let storage = Box::new(crate::storage::MemoryStorage::new());
        let vault_key = crate::crypto::generate_vault_key();
        let device_key = crate::crypto::DeviceKey::generate();
        let mut sync_manager = SyncManager::new(storage, PathBuf::from("/tmp"), vault_key, device_key);
        sync_manager.initialize().await.unwrap();

        // Create two files with identical content
        let temp_dir = tempfile::tempdir().unwrap();
        let file1_path = temp_dir.path().join("file1.txt");
        let file2_path = temp_dir.path().join("file2.txt");
        std::fs::write(&file1_path, "Identical content").unwrap();
        std::fs::write(&file2_path, "Identical content").unwrap();

        // Create two messages with identical files
        let msg1_id = sync_manager.create_message("Message 1".to_string(), vec![file1_path]).await.unwrap();
        let msg2_id = sync_manager.create_message("Message 2".to_string(), vec![file2_path]).await.unwrap();

        // Get attachments
        let attachments1 = sync_manager.storage.get_attachments_for_message(&msg1_id).await.unwrap();
        let attachments2 = sync_manager.storage.get_attachments_for_message(&msg2_id).await.unwrap();

        // Both should have the same file hash
        assert_eq!(attachments1[0].file_hash, attachments2[0].file_hash);

        // Test finding attachments by file hash
        let same_hash_attachments = sync_manager.storage.get_attachments_by_file_hash(&attachments1[0].file_hash).await.unwrap();
        assert_eq!(same_hash_attachments.len(), 2);

        // Test statistics show deduplication
        let stats = sync_manager.storage.get_attachment_stats().await.unwrap();
        assert_eq!(stats.total_attachments, 2);
        assert_eq!(stats.unique_files, 1); // Only one unique file
    }

    #[tokio::test]
    async fn test_progressive_download_functionality() {
        let storage = Box::new(crate::storage::MemoryStorage::new());
        let vault_key = crate::crypto::generate_vault_key();
        let device_key = crate::crypto::DeviceKey::generate();
        let mut sync_manager = SyncManager::new(storage, PathBuf::from("/tmp"), vault_key, device_key);
        sync_manager.initialize().await.unwrap();

        // Create a message with a file attachment
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_progressive.txt");
        let content = b"This is a test file for progressive download functionality. It should be chunked and then reconstructed.";
        tokio::fs::write(&file_path, content).await.unwrap();

        let msg_id = sync_manager.create_message(
            "Message with file for progressive download".to_string(),
            vec![file_path.clone()],
        ).await.unwrap();

        // Get the attachment
        let attachments = sync_manager.storage.get_attachments_for_message(&msg_id).await.unwrap();
        assert_eq!(attachments.len(), 1);
        let attachment = &attachments[0];

        // Check if attachment is available (should be since we just created it)
        let is_available = sync_manager.is_attachment_available(attachment.id).await.unwrap();
        assert!(is_available);

        // Get attachment availability status
        let availability = sync_manager.get_attachment_availability(attachment.id).await.unwrap();
        assert_eq!(availability.attachment_id, attachment.id);
        assert_eq!(availability.filename, "test_progressive.txt");
        assert_eq!(availability.total_size, content.len() as u64);
        assert!(availability.is_fully_available);
        assert_eq!(availability.progress_percentage, 100.0);
        assert_eq!(availability.missing_chunks.len(), 0);

        // Reconstruct the file
        let reconstructed_data = sync_manager.reconstruct_attachment_file(attachment.id).await.unwrap();
        assert_eq!(reconstructed_data, content);

        // Verify file hash matches
        let computed_hash = blake3_hash(&reconstructed_data);
        assert_eq!(computed_hash, attachment.file_hash);

        println!("Progressive download functionality test passed!");
    }
}
