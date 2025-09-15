//! Chunk synchronization implementation for SAVED
//!
//! Handles file attachment chunk storage, synchronization, and distribution
//! between peers using content-addressed storage and convergent encryption.

use crate::crypto::{blake3_hash, derive_chunk_key, VaultKey};
use crate::error::{Error, Result};
use crate::networking::NetworkManager;
use crate::storage::{sqlite::ChunkId, Storage};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Chunk metadata for tracking chunk availability and synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Content identifier (hash of the chunk)
    pub chunk_id: ChunkId,
    /// Size of the chunk in bytes
    pub size: u64,
    /// Timestamp when the chunk was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Whether this chunk is available locally
    pub is_available: bool,
    /// Peers that have this chunk
    pub available_peers: HashSet<String>,
}

/// Chunk synchronization request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkSyncRequest {
    /// Chunk IDs to check availability for
    pub chunk_ids: Vec<ChunkId>,
    /// Requesting peer ID
    pub peer_id: String,
    /// Timestamp of the request
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Chunk synchronization response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkSyncResponse {
    /// Availability bitmap (true = have, false = don't have)
    pub availability: Vec<bool>,
    /// Responding peer ID
    pub peer_id: String,
    /// Timestamp of the response
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Chunk fetch request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkFetchRequest {
    /// Chunk IDs to fetch
    pub chunk_ids: Vec<ChunkId>,
    /// Requesting peer ID
    pub peer_id: String,
    /// Timestamp of the request
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Chunk fetch response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkFetchResponse {
    /// Chunk data (encrypted)
    pub chunks: Vec<ChunkData>,
    /// Responding peer ID
    pub peer_id: String,
    /// Timestamp of the response
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Encrypted chunk data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkData {
    /// Chunk ID
    pub chunk_id: ChunkId,
    /// Encrypted chunk data
    pub encrypted_data: Vec<u8>,
    /// Nonce used for encryption
    pub nonce: Vec<u8>,
}

/// Chunk synchronization manager
pub struct ChunkSyncManager {
    /// Storage backend
    storage: Arc<dyn Storage>,
    /// Vault key for encryption
    vault_key: VaultKey,
    /// Network manager for peer communication
    network_manager: Option<NetworkManager>,
    /// Chunk metadata cache
    chunk_metadata: Arc<RwLock<HashMap<ChunkId, ChunkMetadata>>>,
    /// Peer chunk availability tracking
    peer_availability: Arc<RwLock<HashMap<String, HashSet<ChunkId>>>>,
}

impl ChunkSyncManager {
    /// Create a new chunk synchronization manager
    pub fn new(storage: Arc<dyn Storage>, vault_key: VaultKey) -> Self {
        Self {
            storage,
            vault_key,
            network_manager: None,
            chunk_metadata: Arc::new(RwLock::new(HashMap::new())),
            peer_availability: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set the network manager for peer communication
    pub fn set_network_manager(&mut self, network_manager: NetworkManager) {
        self.network_manager = Some(network_manager);
    }

    /// Store a chunk with convergent encryption
    pub async fn store_chunk(&self, data: &[u8]) -> Result<ChunkId> {
        // Generate chunk ID from content hash
        let chunk_id = blake3_hash(data);

        // Check if chunk already exists
        if self.storage.has_chunk(&chunk_id).await? {
            return Ok(chunk_id);
        }

        // Derive encryption key from content hash (convergent encryption)
        let encryption_key = derive_chunk_key(&self.vault_key, &chunk_id)?;

        // Encrypt the chunk data
        let encrypted_data = self.encrypt_chunk_data(data, &encryption_key)?;

        // Store the encrypted chunk
        self.storage.store_chunk(&chunk_id, &encrypted_data).await?;

        // Update metadata
        let metadata = ChunkMetadata {
            chunk_id,
            size: data.len() as u64,
            created_at: chrono::Utc::now(),
            is_available: true,
            available_peers: HashSet::new(),
        };

        let mut metadata_cache = self.chunk_metadata.write().await;
        metadata_cache.insert(chunk_id, metadata);

        Ok(chunk_id)
    }

    /// Retrieve a chunk by ID
    pub async fn get_chunk(&self, chunk_id: &ChunkId) -> Result<Option<Vec<u8>>> {
        // Check if chunk exists locally
        if !self.storage.has_chunk(chunk_id).await? {
            return Ok(None);
        }

        // Get encrypted data
        let encrypted_data = self
            .storage
            .get_chunk(chunk_id)
            .await?
            .ok_or_else(|| Error::Storage("Chunk not found".to_string()))?;

        // Derive decryption key
        let decryption_key = derive_chunk_key(&self.vault_key, chunk_id)?;

        // Decrypt the chunk data
        let decrypted_data = self.decrypt_chunk_data(&encrypted_data, &decryption_key)?;

        Ok(Some(decrypted_data))
    }

    /// Check chunk availability across peers
    pub async fn check_chunk_availability(
        &self,
        chunk_ids: &[ChunkId],
    ) -> Result<HashMap<ChunkId, bool>> {
        let mut availability = HashMap::new();

        // Check local availability first
        for chunk_id in chunk_ids {
            let is_available = self.storage.has_chunk(chunk_id).await?;
            availability.insert(*chunk_id, is_available);
        }

        // Request availability from peers
        if let Some(_network_manager) = &self.network_manager {
            let request = ChunkSyncRequest {
                chunk_ids: chunk_ids.to_vec(),
                peer_id: "local".to_string(), // TODO: Get actual peer ID
                timestamp: chrono::Utc::now(),
            };

            // Send availability request via gossipsub
            let _data = serde_json::to_vec(&request)
                .map_err(|e| Error::Storage(format!("Failed to serialize request: {}", e)))?;

            // TODO: Send via network manager
            println!(
                "Requesting chunk availability for {} chunks",
                chunk_ids.len()
            );
        }

        Ok(availability)
    }

    /// Fetch missing chunks from peers
    pub async fn fetch_missing_chunks(&self, chunk_ids: &[ChunkId]) -> Result<()> {
        // Check which chunks we need to fetch
        let mut missing_chunks = Vec::new();
        for chunk_id in chunk_ids {
            if !self.storage.has_chunk(chunk_id).await? {
                missing_chunks.push(*chunk_id);
            }
        }

        if missing_chunks.is_empty() {
            return Ok(());
        }

        // Request chunks from peers
        if let Some(_network_manager) = &self.network_manager {
            let request = ChunkFetchRequest {
                chunk_ids: missing_chunks.clone(),
                peer_id: "local".to_string(), // TODO: Get actual peer ID
                timestamp: chrono::Utc::now(),
            };

            // Send fetch request via gossipsub
            let _data = serde_json::to_vec(&request)
                .map_err(|e| Error::Storage(format!("Failed to serialize request: {}", e)))?;

            // TODO: Send via network manager
            println!(
                "Fetching {} missing chunks from peers",
                missing_chunks.len()
            );
        }

        Ok(())
    }

    /// Handle chunk availability request from peer
    pub async fn handle_availability_request(
        &self,
        request: ChunkSyncRequest,
    ) -> Result<ChunkSyncResponse> {
        let mut availability = Vec::new();

        for chunk_id in &request.chunk_ids {
            let is_available = self.storage.has_chunk(chunk_id).await?;
            availability.push(is_available);
        }

        let response = ChunkSyncResponse {
            availability,
            peer_id: "local".to_string(), // TODO: Get actual peer ID
            timestamp: chrono::Utc::now(),
        };

        // Update peer availability tracking
        let mut peer_availability = self.peer_availability.write().await;
        let peer_chunks = peer_availability
            .entry(request.peer_id.clone())
            .or_insert_with(HashSet::new);

        for (i, chunk_id) in request.chunk_ids.iter().enumerate() {
            if i < response.availability.len() && response.availability[i] {
                peer_chunks.insert(*chunk_id);
            }
        }

        Ok(response)
    }

    /// Handle chunk fetch request from peer
    pub async fn handle_fetch_request(
        &self,
        request: ChunkFetchRequest,
    ) -> Result<ChunkFetchResponse> {
        let mut chunks = Vec::new();

        for chunk_id in &request.chunk_ids {
            if let Some(encrypted_data) = self.storage.get_chunk(chunk_id).await? {
                // Derive encryption key for this chunk
                let _encryption_key = derive_chunk_key(&self.vault_key, chunk_id)?;

                // Create chunk data with nonce (for now, use a placeholder)
                let chunk_data = ChunkData {
                    chunk_id: *chunk_id,
                    encrypted_data,
                    nonce: vec![0u8; 24], // TODO: Store actual nonce
                };

                chunks.push(chunk_data);
            }
        }

        let response = ChunkFetchResponse {
            chunks,
            peer_id: "local".to_string(), // TODO: Get actual peer ID
            timestamp: chrono::Utc::now(),
        };

        Ok(response)
    }

    /// Handle chunk fetch response from peer
    pub async fn handle_fetch_response(&self, response: ChunkFetchResponse) -> Result<()> {
        for chunk_data in &response.chunks {
            // Store the encrypted chunk data
            self.storage
                .store_chunk(&chunk_data.chunk_id, &chunk_data.encrypted_data)
                .await?;

            // Update metadata
            let mut metadata_cache = self.chunk_metadata.write().await;
            if let Some(metadata) = metadata_cache.get_mut(&chunk_data.chunk_id) {
                metadata.is_available = true;
                metadata.available_peers.insert(response.peer_id.clone());
            } else {
                let metadata = ChunkMetadata {
                    chunk_id: chunk_data.chunk_id,
                    size: chunk_data.encrypted_data.len() as u64,
                    created_at: chrono::Utc::now(),
                    is_available: true,
                    available_peers: {
                        let mut peers = HashSet::new();
                        peers.insert(response.peer_id.clone());
                        peers
                    },
                };
                metadata_cache.insert(chunk_data.chunk_id, metadata);
            }
        }

        Ok(())
    }

    /// Get chunk metadata
    pub async fn get_chunk_metadata(&self, chunk_id: &ChunkId) -> Result<Option<ChunkMetadata>> {
        let metadata_cache = self.chunk_metadata.read().await;
        Ok(metadata_cache.get(chunk_id).cloned())
    }

    /// Get all chunk metadata
    pub async fn get_all_chunk_metadata(&self) -> Result<Vec<ChunkMetadata>> {
        let metadata_cache = self.chunk_metadata.read().await;
        Ok(metadata_cache.values().cloned().collect())
    }

    /// Encrypt chunk data
    fn encrypt_chunk_data(&self, data: &[u8], _key: &[u8; 32]) -> Result<Vec<u8>> {
        // TODO: Implement proper XChaCha20-Poly1305 encryption
        // For now, return the data as-is (placeholder)
        Ok(data.to_vec())
    }

    /// Decrypt chunk data
    fn decrypt_chunk_data(&self, encrypted_data: &[u8], _key: &[u8; 32]) -> Result<Vec<u8>> {
        // TODO: Implement proper XChaCha20-Poly1305 decryption
        // For now, return the data as-is (placeholder)
        Ok(encrypted_data.to_vec())
    }

    /// Get peer availability for a chunk
    pub async fn get_chunk_peers(&self, chunk_id: &ChunkId) -> Result<HashSet<String>> {
        let peer_availability = self.peer_availability.read().await;
        let mut peers = HashSet::new();

        for (peer_id, peer_chunks) in peer_availability.iter() {
            if peer_chunks.contains(chunk_id) {
                peers.insert(peer_id.clone());
            }
        }

        Ok(peers)
    }

    /// Clean up old chunk metadata
    pub async fn cleanup_old_metadata(&self, max_age_days: u64) -> Result<()> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(max_age_days as i64);
        let mut metadata_cache = self.chunk_metadata.write().await;

        metadata_cache.retain(|_, metadata| metadata.created_at > cutoff);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    #[tokio::test]
    async fn test_chunk_storage_and_retrieval() {
        let storage = Arc::new(MemoryStorage::new());
        let vault_key = [1u8; 32];
        let manager = ChunkSyncManager::new(storage, vault_key);

        let test_data = b"Hello, World!";
        let chunk_id = manager.store_chunk(test_data).await.unwrap();

        let retrieved_data = manager.get_chunk(&chunk_id).await.unwrap().unwrap();
        assert_eq!(test_data, retrieved_data.as_slice());
    }

    #[tokio::test]
    async fn test_chunk_availability() {
        let storage = Arc::new(MemoryStorage::new());
        let vault_key = [1u8; 32];
        let manager = ChunkSyncManager::new(storage, vault_key);

        let test_data = b"Test chunk data";
        let chunk_id = manager.store_chunk(test_data).await.unwrap();

        let availability = manager.check_chunk_availability(&[chunk_id]).await.unwrap();
        assert!(availability.get(&chunk_id).unwrap());
    }

    #[tokio::test]
    async fn test_chunk_metadata() {
        let storage = Arc::new(MemoryStorage::new());
        let vault_key = [1u8; 32];
        let manager = ChunkSyncManager::new(storage, vault_key);

        let test_data = b"Metadata test data";
        let chunk_id = manager.store_chunk(test_data).await.unwrap();

        let metadata = manager
            .get_chunk_metadata(&chunk_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(metadata.chunk_id, chunk_id);
        assert_eq!(metadata.size, test_data.len() as u64);
        assert!(metadata.is_available);
    }
}
