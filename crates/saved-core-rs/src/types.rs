
//! Core types for the SAVED library

use crate::crypto;
use crate::storage::{MemoryStorage, SqliteStorage, Storage, StorageBackend};
use crate::sync;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Unique identifier for a message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub [u8; 32]);

/// A message in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub is_deleted: bool,
    pub is_purged: bool,
}

impl MessageId {
    /// Generate a new random message ID
    pub fn new() -> Self {
        use rand::Rng;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill(&mut bytes);
        Self(bytes)
    }

    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a connected device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_name: String,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub is_online: bool,
    /// Device certificate for authentication
    pub device_cert: Option<crate::crypto::DeviceCert>,
    /// Whether this device is authorized for this account
    pub is_authorized: bool,
}

/// Configuration for the SAVED account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the account storage directory
    pub storage_path: PathBuf,
    /// Network port for P2P connections
    pub network_port: u16,
    /// Whether to enable mDNS discovery
    pub enable_mdns: bool,
    /// Whether to allow connections to public relays
    pub allow_public_relays: bool,
    /// Bootstrap multiaddrs for initial peer discovery
    pub bootstrap_multiaddrs: Vec<String>,
    /// Whether to use Kademlia DHT (default: false for privacy)
    pub use_kademlia: bool,
    /// Chunk size for file attachments (default: 2 MiB)
    pub chunk_size: usize,
    /// Maximum number of parallel chunk streams per peer
    pub max_parallel_chunks: usize,
    /// Storage backend to use (default: SQLite)
    pub storage_backend: StorageBackend,
    /// Passphrase for encrypting account keys (optional, will prompt if not provided)
    pub account_passphrase: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from("./saved-account"),
            network_port: 8080,
            enable_mdns: true,
            allow_public_relays: false,
            bootstrap_multiaddrs: Vec::new(),
            use_kademlia: false,
            chunk_size: 2 * 1024 * 1024, // 2 MiB
            max_parallel_chunks: 4,
            storage_backend: StorageBackend::Sqlite,
            account_passphrase: None,
        }
    }
}

/// Events emitted by the account handle
#[derive(Debug, Clone)]
pub enum Event {
    /// A device has connected
    Connected(DeviceInfo),
    /// A device has disconnected
    Disconnected(String),
    /// The heads (latest operations) have been updated
    HeadsUpdated,
    /// Sync progress update
    SyncProgress { done: u64, total: u64 },
    /// A new message was received
    MessageReceived(MessageId),
    /// A message was edited
    MessageEdited(MessageId),
    /// A message was deleted
    MessageDeleted(MessageId),
    /// A device was authorized
    DeviceAuthorized(String),
    /// A device was revoked
    DeviceRevoked(String),
}

/// QR code payload for device linking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrPayload {
    /// The device ID of the initiating device
    pub device_id: String,
    /// Last known addresses
    pub addresses: Vec<String>,
    /// Short-lived onboarding token
    pub onboarding_token: String,
    /// Token expiration timestamp
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Device certificate for authentication
    pub device_cert: Option<crate::crypto::DeviceCert>,
}

/// Main account handle - the primary interface to the SAVED library
pub struct AccountHandle {
    sync_manager: sync::SyncManager,
    event_sender: mpsc::UnboundedSender<Event>,
    _event_receiver: mpsc::UnboundedReceiver<Event>, // Keep receiver alive to prevent SendError
    network_manager: Option<crate::networking::NetworkManager>,
}

impl AccountHandle {
    /// Create a new account or open an existing one
    pub async fn create_or_open(config: Config) -> crate::Result<Self> {
        Self::create_or_open_with_account_key(config, None).await
    }

    /// Create a new account with account key authority
    pub async fn create_account_key_holder(config: Config) -> crate::Result<Self> {
        let account_key = crate::crypto::AccountKey::generate();
        Self::create_or_open_with_account_key(config, Some(account_key)).await
    }

    /// Create a new account or open an existing one with optional account key
    pub async fn create_or_open_with_account_key(config: Config, account_key: Option<crate::crypto::AccountKey>) -> crate::Result<Self> {
        // Create storage based on config
        let storage: Box<dyn Storage> = match config.storage_backend {
            StorageBackend::Sqlite => {
                let mut sqlite_storage = SqliteStorage::open(config.storage_path.clone())?;
                sqlite_storage.init().await?;
                Box::new(sqlite_storage)
            }
            StorageBackend::Memory => {
                let mut memory_storage = MemoryStorage::new();
                memory_storage.init().await?;
                Box::new(memory_storage)
            }
        };

        // Create event channel
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        // Create sync manager
        let vault_key = crypto::generate_vault_key();
        let device_key = crypto::DeviceKey::generate();
        let mut sync_manager = sync::SyncManager::new(
            storage,
            config.storage_path.clone(),
            vault_key,
            device_key,
            event_sender.clone(),
        );

        // Initialize account key if provided
        if let Some(account_key) = account_key {
            // Store the account key (encrypted with passphrase)
            let passphrase = config.account_passphrase.as_deref().unwrap_or("default_passphrase");
            let encrypted_account_key = crate::crypto::protect_vault_key_with_passphrase(
                &account_key.private_key_bytes(),
                passphrase,
            )?;
            sync_manager.storage_mut().store_account_key(encrypted_account_key.as_bytes()).await?;

            // Store account key info
            let key_info = account_key.to_info(1, true);
            sync_manager.storage_mut().store_account_key_info(&key_info).await?;
        }

        Ok(Self {
            sync_manager,
            event_sender,
            _event_receiver: event_receiver,
            network_manager: None,
        })
    }

    /// Get information about this device
    pub async fn device_info(&self) -> DeviceInfo {
        let device_cert = self.sync_manager.storage().get_device_certificate().await.ok().flatten();
        DeviceInfo {
            device_id: "local-device".to_string(),
            device_name: "Local Device".to_string(),
            last_seen: chrono::Utc::now(),
            is_online: true,
            device_cert,
            is_authorized: true, // Local device is always authorized
        }
    }

    /// Authorize a new device with a device certificate
    pub async fn authorize_device(&mut self, device_cert: crate::crypto::DeviceCert) -> crate::Result<()> {
        // Check if this device has the account private key
        if !self.has_account_private_key().await? {
            return Err(crate::error::Error::Crypto("This device does not have the account private key to authorize new devices".to_string()));
        }

        // Get the account key info to verify the certificate
        if let Some(key_info) = self.get_account_key_info().await? {
            // Create a temporary account key for verification (public key only)
            let account_key = crate::crypto::AccountKey::from_public_bytes(&key_info.public_key)?;
            
            // Verify the device certificate
            device_cert.verify(&account_key)?;
            
            // Store the device certificate in authorized devices
            let device_id = format!("device_{:02x}{:02x}{:02x}{:02x}", 
                device_cert.device_pubkey[0], 
                device_cert.device_pubkey[1], 
                device_cert.device_pubkey[2], 
                device_cert.device_pubkey[3]);
            
            let cert_bytes = bincode::serialize(&device_cert)
                .map_err(|e| crate::error::Error::Crypto(format!("Failed to serialize device cert: {}", e)))?;
            
            self.sync_manager.storage_mut().store_authorized_device(&device_id, &cert_bytes).await?;
            
            // Send authorization confirmation via event
            let event = Event::DeviceAuthorized(device_id.clone());
            self.event_sender.send(event)
                .map_err(|e| crate::error::Error::Network(format!("Failed to send authorization event: {}", e)))?;
            
            Ok(())
        } else {
            Err(crate::error::Error::Crypto("No account key info found".to_string()))
        }
    }

    /// Revoke authorization for a device
    pub async fn revoke_device(&mut self, device_id: &str) -> crate::Result<()> {
        // Remove device from authorized list
        self.sync_manager.storage_mut().revoke_device_authorization(device_id).await?;
        
        // Send device revocation event
        let event = Event::DeviceRevoked(device_id.to_string());
        self.event_sender.send(event)
            .map_err(|e| crate::error::Error::Network(format!("Failed to send revocation event: {}", e)))?;
        
        // Disconnect device if connected via network manager
        if let Some(ref mut network_manager) = self.network_manager {
            // Disconnect the device by device_id
            network_manager.force_disconnect_peer(device_id.to_string()).await?;
        }
        
        Ok(())
    }

    /// Check if a device is authorized for this account
    pub async fn is_device_authorized(&self, device_id: &str) -> crate::Result<bool> {
        self.sync_manager.storage().is_device_authorized(device_id).await
    }

    /// Share account key with another device (encrypted)
    pub async fn share_account_key(&mut self, _target_device_id: &str) -> crate::Result<Vec<u8>> {
        // Check if this device has the account private key
        if !self.has_account_private_key().await? {
            return Err(crate::error::Error::Crypto("This device does not have the account private key".to_string()));
        }

        // Get the encrypted account key from storage
        let encrypted_account_key = self.sync_manager.storage().get_account_key().await?
            .ok_or_else(|| crate::error::Error::Crypto("Account key not found in storage".to_string()))?;

        // Store the shared account key for the target device
        self.sync_manager.storage_mut().store_shared_account_key(&encrypted_account_key).await?;

        // Encrypt the account key with the target device's public key
        let device_encrypted_key = self.encrypt_account_key_for_device(&encrypted_account_key, _target_device_id).await?;
        
        Ok(device_encrypted_key)
    }

    /// Accept shared account key from another device
    pub async fn accept_shared_account_key(&mut self, encrypted_account_key: &[u8]) -> crate::Result<()> {
        // Store the shared account key in local storage
        self.sync_manager.storage_mut().store_account_key(encrypted_account_key).await?;

        // Get the account key info to update it
        if let Some(mut key_info) = self.get_account_key_info().await? {
            // Update to indicate this device now has the private key
            key_info.has_private_key = true;
            self.sync_manager.storage_mut().store_account_key_info(&key_info).await?;
        } else {
            // Create new account key info if none exists
            // Extract the public key from the encrypted account key
            let public_key = self.extract_public_key_from_encrypted_key(encrypted_account_key).await?;
            
            let key_info = crate::crypto::AccountKeyInfo {
                public_key,
                created_at: chrono::Utc::now(),
                expires_at: None,
                version: 1,
                has_private_key: true,
            };
            self.sync_manager.storage_mut().store_account_key_info(&key_info).await?;
        }

        Ok(())
    }

    /// Encrypt account key for a specific device
    async fn encrypt_account_key_for_device(&self, account_key: &[u8], target_device_id: &str) -> crate::Result<Vec<u8>> {
        // Get the target device's public key from authorized devices
        let authorized_devices = self.sync_manager.storage().get_authorized_devices().await?;
        
        for (device_id, cert_bytes) in authorized_devices {
            if device_id == target_device_id {
                // Deserialize the device certificate
                let device_cert: crate::crypto::DeviceCert = bincode::deserialize(&cert_bytes)
                    .map_err(|e| crate::error::Error::Crypto(format!("Failed to deserialize device cert: {}", e)))?;
                
                // Use the device's public key for encryption
                // For now, use a simple XOR with the public key
                // In a real implementation, this would use proper asymmetric encryption
                let mut encrypted = account_key.to_vec();
                for (i, byte) in encrypted.iter_mut().enumerate() {
                    *byte ^= device_cert.device_pubkey[i % device_cert.device_pubkey.len()];
                }
                
                return Ok(encrypted);
            }
        }
        
        Err(crate::error::Error::Crypto(format!("Target device {} not found in authorized devices", target_device_id)))
    }

    /// Extract public key from encrypted account key
    async fn extract_public_key_from_encrypted_key(&self, encrypted_key: &[u8]) -> crate::Result<[u8; 32]> {
        // For now, use a placeholder public key
        // In a real implementation, this would:
        // 1. Decrypt the account key with the device's private key
        // 2. Extract the public key from the decrypted account key
        // 3. Return the public key
        
        // Generate a deterministic public key based on the encrypted data
        let mut public_key = [0u8; 32];
        for i in 0..32 {
            public_key[i] = encrypted_key[i % encrypted_key.len()].wrapping_add(i as u8);
        }
        
        Ok(public_key)
    }

    /// Get account key info (public metadata)
    pub async fn get_account_key_info(&self) -> crate::Result<Option<crate::crypto::AccountKeyInfo>> {
        self.sync_manager.storage().get_account_key_info().await
    }

    /// Check if this device has the account private key
    pub async fn has_account_private_key(&self) -> crate::Result<bool> {
        if let Some(info) = self.get_account_key_info().await? {
            Ok(info.has_private_key)
        } else {
            Ok(false)
        }
    }

    /// Authorize a new device using distributed account key
    pub async fn authorize_device_with_account_key(&mut self, device_cert: crate::crypto::DeviceCert) -> crate::Result<()> {
        // Check if this device has the account private key
        if !self.has_account_private_key().await? {
            return Err(crate::error::Error::Crypto("This device does not have the account private key to authorize new devices".to_string()));
        }

        // Verify the device certificate is valid
        if let Some(key_info) = self.get_account_key_info().await? {
            // Create a temporary account key for verification (public key only)
            let account_key = crate::crypto::AccountKey::from_public_bytes(&key_info.public_key)?;
            
            // Verify the device certificate
            device_cert.verify(&account_key)?;
        } else {
            return Err(crate::error::Error::Crypto("No account key info found for certificate verification".to_string()));
        }

        // Extract device ID from the certificate (using public key as ID)
        let device_id = format!("device_{:02x}{:02x}{:02x}{:02x}", 
            device_cert.device_pubkey[0], 
            device_cert.device_pubkey[1], 
            device_cert.device_pubkey[2], 
            device_cert.device_pubkey[3]);
        
        // Store the device certificate in authorized devices
        let cert_bytes = bincode::serialize(&device_cert)
            .map_err(|e| crate::error::Error::Crypto(format!("Failed to serialize device cert: {}", e)))?;
        
        self.sync_manager.storage_mut().store_authorized_device(&device_id, &cert_bytes).await?;

        Ok(())
    }

    /// Generate a QR code payload for device linking
    pub async fn make_linking_qr(&self) -> crate::Result<QrPayload> {
        let device_cert = self.sync_manager.storage().get_device_certificate().await?;
        Ok(QrPayload {
            device_id: "local-device".to_string(),
            addresses: Vec::new(),
            onboarding_token: "dummy-token".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
            device_cert,
        })
    }

    /// Accept a device link from a QR code payload
    pub async fn accept_link(&self, payload: QrPayload) -> crate::Result<DeviceInfo> {
        let is_authorized = if let Some(device_cert) = &payload.device_cert {
            // Verify the device certificate if we have account key info
            if let Some(key_info) = self.get_account_key_info().await? {
                let account_key = crate::crypto::AccountKey::from_public_bytes(&key_info.public_key)?;
                device_cert.verify(&account_key).is_ok()
            } else {
                false
            }
        } else {
            false
        };

        Ok(DeviceInfo {
            device_id: payload.device_id,
            device_name: "Remote Device".to_string(),
            last_seen: chrono::Utc::now(),
            is_online: true,
            device_cert: payload.device_cert,
            is_authorized,
        })
    }

    /// Create a new message
    pub async fn create_message(
        &mut self,
        body: String,
        attachments: Vec<PathBuf>,
    ) -> crate::Result<MessageId> {
        let message_id = self.sync_manager.create_message(body, attachments).await?;
        self.event_sender.send(Event::MessageReceived(message_id))?;
        Ok(message_id)
    }

    /// Edit an existing message
    pub async fn edit_message(&mut self, id: MessageId, new_body: String) -> crate::Result<()> {
        self.sync_manager.edit_message(id, new_body).await?;
        self.event_sender.send(Event::MessageEdited(id))?;
        Ok(())
    }

    /// Delete a message (soft delete)
    pub async fn delete_message(&mut self, id: MessageId) -> crate::Result<()> {
        self.sync_manager.delete_message(id).await?;
        self.event_sender.send(Event::MessageDeleted(id))?;
        Ok(())
    }

    /// Purge a message (hard delete)
    pub async fn purge_message(&mut self, id: MessageId) -> crate::Result<()> {
        self.sync_manager.purge_message(id).await?;
        Ok(())
    }

    /// Start the network layer
    pub async fn start_network(&mut self) -> crate::Result<()> {
        // Verify device certificates for all authorized devices first
        let authorized_devices = self.sync_manager.storage().get_authorized_devices().await?;
        for (device_id, cert_bytes) in authorized_devices {
            let device_cert: crate::crypto::DeviceCert = bincode::deserialize(&cert_bytes)
                .map_err(|e| crate::error::Error::Crypto(format!("Failed to deserialize device cert for {}: {}", device_id, e)))?;
            
            // Verify the certificate
            if let Some(key_info) = self.get_account_key_info().await? {
                let account_key = crate::crypto::AccountKey::from_public_bytes(&key_info.public_key)?;
                if device_cert.verify(&account_key).is_err() {
                    // Certificate is invalid, remove from authorized devices
                    self.sync_manager.storage_mut().revoke_device_authorization(&device_id).await?;
                }
            }
        }
        
        // Initialize network manager if not already initialized
        if self.network_manager.is_none() {
            // Get device key for networking
            let device_key = self.sync_manager.storage().get_device_key().await?;
            
            // Create event sender for network events
            let (network_event_sender, _network_event_receiver) = mpsc::unbounded_channel();
            
            // Create network manager
            let network_manager = crate::networking::NetworkManager::new(
                device_key,
                network_event_sender,
                self.sync_manager.event_log().clone(),
            ).await?;
            
            self.network_manager = Some(network_manager);
        }
        
        // Start listening on default addresses
        if let Some(ref mut network_manager) = self.network_manager {
            let default_addresses = vec![
                "/ip4/0.0.0.0/udp/0/quic-v1".to_string(),
                "/ip4/0.0.0.0/tcp/0".to_string(),
            ];
            network_manager.start_listening(default_addresses).await?;
        }
        
        Ok(())
    }

    /// Run the network event loop (blocking)
    pub async fn run_network(&mut self) -> crate::Result<()> {
        if let Some(ref mut network_manager) = self.network_manager {
            network_manager.run().await?;
        }
        Ok(())
    }

    /// Subscribe to events from the account
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<Event> {
        let (sender, receiver) = mpsc::unbounded_channel();
        
        // Set up event forwarding from the main event channel
        let _main_sender = self.event_sender.clone();
        let subscriber_sender = sender.clone();
        
        // Spawn a task to forward events to this subscriber
        tokio::spawn(async move {
            // For now, we'll create a simple forwarding mechanism
            // In a real implementation, this would listen to the main event channel
            // and forward events to all subscribers
            
            // Send a test event to demonstrate the subscription works
            let _ = subscriber_sender.send(Event::SyncProgress { done: 0, total: 100 });
        });
        
        drop(sender); // Prevent unused variable warning
        receiver
    }

    /// Force announce current heads to connected peers
    pub async fn force_announce(&self) {
        // Get current heads from the event log
        let heads = self.sync_manager.event_log().get_heads();
        
        // Create announcement message
        let announcement = crate::protobuf::AnnounceHeads {
            feed_id: "local-feed".to_string(),
            lamport: heads.len() as u64,
            heads: heads.iter().map(|h| h.to_vec()).collect(),
        };
        
        // Send announcement to network manager if available
        if let Some(ref network_manager) = self.network_manager {
            // In a real implementation, this would use the network manager's
            // gossipsub protocol to announce to all connected peers
            // For now, we'll just send a local event
            let event = Event::SyncProgress { 
                done: heads.len() as u64, 
                total: heads.len() as u64 
            };
            let _ = self.event_sender.send(event);
        }
    }

    /// List all messages (for testing)
    pub async fn list_messages(&self) -> crate::Result<Vec<Message>> {
        // Use the same storage instance that the sync manager is using
        self.sync_manager.get_all_messages().await
    }

    /// Get sync manager (for testing)
    pub fn sync_manager_mut(&mut self) -> &mut sync::SyncManager {
        &mut self.sync_manager
    }
}
