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
    account_key: Arc<RwLock<Option<Vec<u8>>>>,
    account_key_info: Arc<RwLock<Option<crate::crypto::AccountKeyInfo>>>,
    shared_account_key: Arc<RwLock<Option<Vec<u8>>>>,
    vault_key: Arc<RwLock<Option<Vec<u8>>>>,
    authorized_devices: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    device_certificate: Arc<RwLock<Option<crate::crypto::DeviceCert>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(Vec::new())),
            messages: Arc::new(RwLock::new(HashMap::new())),
            chunks: Arc::new(RwLock::new(HashMap::new())),
            account_key: Arc::new(RwLock::new(None)),
            account_key_info: Arc::new(RwLock::new(None)),
            shared_account_key: Arc::new(RwLock::new(None)),
            vault_key: Arc::new(RwLock::new(None)),
            authorized_devices: Arc::new(RwLock::new(HashMap::new())),
            device_certificate: Arc::new(RwLock::new(None)),
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

    async fn store_account_key(&self, encrypted_account_key: &[u8]) -> Result<()> {
        let mut account_key = self.account_key.write().await;
        *account_key = Some(encrypted_account_key.to_vec());
        Ok(())
    }

    async fn get_account_key(&self) -> Result<Option<Vec<u8>>> {
        let account_key = self.account_key.read().await;
        Ok(account_key.clone())
    }

    async fn store_account_key_info(&self, key_info: &crate::crypto::AccountKeyInfo) -> Result<()> {
        let mut info = self.account_key_info.write().await;
        *info = Some(key_info.clone());
        Ok(())
    }

    async fn get_account_key_info(&self) -> Result<Option<crate::crypto::AccountKeyInfo>> {
        let info = self.account_key_info.read().await;
        Ok(info.clone())
    }

    async fn store_shared_account_key(&self, encrypted_account_key: &[u8]) -> Result<()> {
        let mut shared_key = self.shared_account_key.write().await;
        *shared_key = Some(encrypted_account_key.to_vec());
        Ok(())
    }

    async fn get_shared_account_key(&self) -> Result<Option<Vec<u8>>> {
        let shared_key = self.shared_account_key.read().await;
        Ok(shared_key.clone())
    }

    async fn store_vault_key(&self, encrypted_vault_key: &[u8]) -> Result<()> {
        let mut vault_key = self.vault_key.write().await;
        *vault_key = Some(encrypted_vault_key.to_vec());
        Ok(())
    }

    async fn get_vault_key(&self) -> Result<Option<Vec<u8>>> {
        let vault_key = self.vault_key.read().await;
        Ok(vault_key.clone())
    }

    async fn store_authorized_device(&self, device_id: &str, device_cert: &[u8]) -> Result<()> {
        let mut devices = self.authorized_devices.write().await;
        devices.insert(device_id.to_string(), device_cert.to_vec());
        Ok(())
    }

    async fn get_authorized_devices(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let devices = self.authorized_devices.read().await;
        Ok(devices
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    async fn revoke_device_authorization(&self, device_id: &str) -> Result<()> {
        let mut devices = self.authorized_devices.write().await;
        devices.remove(device_id);
        Ok(())
    }

    async fn is_device_authorized(&self, device_id: &str) -> Result<bool> {
        let devices = self.authorized_devices.read().await;
        Ok(devices.contains_key(device_id))
    }

    async fn store_device_certificate(&self, device_cert: &crate::crypto::DeviceCert) -> Result<()> {
        let mut cert = self.device_certificate.write().await;
        *cert = Some(device_cert.clone());
        Ok(())
    }

    async fn get_device_certificate(&self) -> Result<Option<crate::crypto::DeviceCert>> {
        let cert = self.device_certificate.read().await;
        Ok(cert.clone())
    }

    async fn get_device_key(&self) -> Result<crate::crypto::DeviceKey> {
        // For now, generate a new device key
        // In a real implementation, this would be stored and retrieved from storage
        Ok(crate::crypto::DeviceKey::generate())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_account_key_storage() {
        let storage = MemoryStorage::new();

        // Initially no account key
        assert!(storage.get_account_key().await.unwrap().is_none());

        // Store an encrypted account key
        let encrypted_key = vec![1, 2, 3, 4, 5];
        storage.store_account_key(&encrypted_key).await.unwrap();

        // Retrieve the account key
        let retrieved = storage.get_account_key().await.unwrap();
        assert_eq!(retrieved, Some(encrypted_key));
    }

    #[tokio::test]
    async fn test_vault_key_storage() {
        let storage = MemoryStorage::new();

        // Initially no vault key
        assert!(storage.get_vault_key().await.unwrap().is_none());

        // Store an encrypted vault key
        let encrypted_key = vec![6, 7, 8, 9, 10];
        storage.store_vault_key(&encrypted_key).await.unwrap();

        // Retrieve the vault key
        let retrieved = storage.get_vault_key().await.unwrap();
        assert_eq!(retrieved, Some(encrypted_key));
    }

    #[tokio::test]
    async fn test_key_overwrite() {
        let storage = MemoryStorage::new();

        // Store initial keys
        let key1 = vec![1, 2, 3];
        let key2 = vec![4, 5, 6];
        storage.store_account_key(&key1).await.unwrap();
        storage.store_vault_key(&key2).await.unwrap();

        // Overwrite with new keys
        let key3 = vec![7, 8, 9];
        let key4 = vec![10, 11, 12];
        storage.store_account_key(&key3).await.unwrap();
        storage.store_vault_key(&key4).await.unwrap();

        // Verify overwrite
        assert_eq!(storage.get_account_key().await.unwrap(), Some(key3));
        assert_eq!(storage.get_vault_key().await.unwrap(), Some(key4));
    }

    #[tokio::test]
    async fn test_device_authorization() {
        let storage = MemoryStorage::new();

        // Initially no authorized devices
        assert!(!storage.is_device_authorized("device1").await.unwrap());
        assert_eq!(storage.get_authorized_devices().await.unwrap().len(), 0);

        // Authorize a device
        let device_cert = vec![1, 2, 3, 4, 5];
        storage
            .store_authorized_device("device1", &device_cert)
            .await
            .unwrap();

        // Check authorization
        assert!(storage.is_device_authorized("device1").await.unwrap());
        assert!(!storage.is_device_authorized("device2").await.unwrap());

        // Get authorized devices
        let devices = storage.get_authorized_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].0, "device1");
        assert_eq!(devices[0].1, device_cert);

        // Revoke authorization
        storage
            .revoke_device_authorization("device1")
            .await
            .unwrap();
        assert!(!storage.is_device_authorized("device1").await.unwrap());
        assert_eq!(storage.get_authorized_devices().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_multiple_device_authorization() {
        let storage = MemoryStorage::new();

        // Authorize multiple devices
        let cert1 = vec![1, 2, 3];
        let cert2 = vec![4, 5, 6];
        let cert3 = vec![7, 8, 9];

        storage
            .store_authorized_device("device1", &cert1)
            .await
            .unwrap();
        storage
            .store_authorized_device("device2", &cert2)
            .await
            .unwrap();
        storage
            .store_authorized_device("device3", &cert3)
            .await
            .unwrap();

        // Check all are authorized
        assert!(storage.is_device_authorized("device1").await.unwrap());
        assert!(storage.is_device_authorized("device2").await.unwrap());
        assert!(storage.is_device_authorized("device3").await.unwrap());

        // Get all devices
        let devices = storage.get_authorized_devices().await.unwrap();
        assert_eq!(devices.len(), 3);

        // Revoke one device
        storage
            .revoke_device_authorization("device2")
            .await
            .unwrap();
        assert!(storage.is_device_authorized("device1").await.unwrap());
        assert!(!storage.is_device_authorized("device2").await.unwrap());
        assert!(storage.is_device_authorized("device3").await.unwrap());

        // Check remaining devices
        let devices = storage.get_authorized_devices().await.unwrap();
        assert_eq!(devices.len(), 2);
    }

    #[tokio::test]
    async fn test_account_key_info_storage() {
        let storage = MemoryStorage::new();
        
        // Initially no account key info
        assert!(storage.get_account_key_info().await.unwrap().is_none());
        
        // Store account key info
        let key_info = crate::crypto::AccountKeyInfo {
            public_key: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32],
            created_at: chrono::Utc::now(),
            expires_at: None,
            version: 1,
            has_private_key: true,
        };
        
        storage.store_account_key_info(&key_info).await.unwrap();
        
        // Retrieve and verify
        let retrieved = storage.get_account_key_info().await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.public_key, key_info.public_key);
        assert_eq!(retrieved.version, 1);
        assert!(retrieved.has_private_key);
    }

    #[tokio::test]
    async fn test_shared_account_key_storage() {
        let storage = MemoryStorage::new();
        
        // Initially no shared account key
        assert!(storage.get_shared_account_key().await.unwrap().is_none());
        
        // Store shared account key
        let encrypted_key = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        storage.store_shared_account_key(&encrypted_key).await.unwrap();
        
        // Retrieve and verify
        let retrieved = storage.get_shared_account_key().await.unwrap();
        assert_eq!(retrieved, Some(encrypted_key));
        
        // Overwrite with new key
        let new_encrypted_key = vec![11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
        storage.store_shared_account_key(&new_encrypted_key).await.unwrap();
        
        let retrieved = storage.get_shared_account_key().await.unwrap();
        assert_eq!(retrieved, Some(new_encrypted_key));
    }

    #[tokio::test]
    async fn test_account_key_info_with_expiration() {
        let storage = MemoryStorage::new();
        
        let expires_at = chrono::Utc::now() + chrono::Duration::days(30);
        let key_info = crate::crypto::AccountKeyInfo {
            public_key: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32],
            created_at: chrono::Utc::now(),
            expires_at: Some(expires_at),
            version: 2,
            has_private_key: false,
        };
        
        storage.store_account_key_info(&key_info).await.unwrap();
        
        let retrieved = storage.get_account_key_info().await.unwrap().unwrap();
        assert_eq!(retrieved.version, 2);
        assert!(!retrieved.has_private_key);
        assert!(retrieved.expires_at.is_some());
    }
}
