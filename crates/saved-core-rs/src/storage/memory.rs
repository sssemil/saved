use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::trait_impl::{Storage, StorageStats};
use crate::error::Result;
use crate::events::Op;
use crate::types::{Message, MessageId};

/// In-memory storage implementation
pub struct MemoryStorage {
    operations: Arc<RwLock<Vec<Op>>>,
    messages: Arc<RwLock<HashMap<MessageId, Message>>>,
    chunks: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(Vec::new())),
            messages: Arc::new(RwLock::new(HashMap::new())),
            chunks: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn init(&mut self) -> Result<()> {
        // No initialization needed for in-memory storage
        Ok(())
    }

    async fn store_operation(&self, operation: &Op) -> Result<()> {
        let mut ops = self.operations.write().await;
        ops.push(operation.clone());
        Ok(())
    }

    async fn get_all_operations(&self) -> Result<Vec<Op>> {
        let ops = self.operations.read().await;
        Ok(ops.clone())
    }

    async fn get_device_operations(&self, device_id: &[u8; 32]) -> Result<Vec<Op>> {
        let ops = self.operations.read().await;
        Ok(ops
            .iter()
            .filter(|op| op.id.device_pubkey == *device_id)
            .cloned()
            .collect())
    }

    async fn store_chunk(&self, hash: &[u8; 32], data: &[u8]) -> Result<()> {
        let mut chunks = self.chunks.write().await;
        chunks.insert(*hash, data.to_vec());
        Ok(())
    }

    async fn get_chunk(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let chunks = self.chunks.read().await;
        Ok(chunks.get(hash).cloned())
    }

    async fn has_chunk(&self, hash: &[u8; 32]) -> Result<bool> {
        let chunks = self.chunks.read().await;
        Ok(chunks.contains_key(hash))
    }

    async fn get_all_messages(&self) -> Result<Vec<Message>> {
        let messages = self.messages.read().await;
        // Filter out deleted messages
        Ok(messages
            .values()
            .filter(|msg| !msg.is_deleted)
            .cloned()
            .collect())
    }

    async fn get_message(&self, message_id: &MessageId) -> Result<Option<Message>> {
        let messages = self.messages.read().await;
        Ok(messages.get(message_id).cloned())
    }

    async fn store_message(&self, message: &Message) -> Result<()> {
        let mut messages = self.messages.write().await;
        messages.insert(message.id, message.clone());
        Ok(())
    }

    async fn delete_message(&self, message_id: &MessageId) -> Result<()> {
        let mut messages = self.messages.write().await;
        messages.remove(message_id);
        Ok(())
    }

    async fn get_stats(&self) -> Result<StorageStats> {
        let ops = self.operations.read().await;
        let messages = self.messages.read().await;
        let chunks = self.chunks.read().await;

        let total_size = chunks.values().map(|data| data.len() as u64).sum();

        Ok(StorageStats {
            operation_count: ops.len(),
            message_count: messages.len(),
            chunk_count: chunks.len(),
            total_size,
        })
    }
}
