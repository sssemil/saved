
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
}

/// Main account handle - the primary interface to the SAVED library
pub struct AccountHandle {
    sync_manager: sync::SyncManager,
    event_sender: mpsc::UnboundedSender<Event>,
    _event_receiver: mpsc::UnboundedReceiver<Event>, // Keep receiver alive to prevent SendError
}

impl AccountHandle {
    /// Create a new account or open an existing one
    pub async fn create_or_open(config: Config) -> crate::Result<Self> {
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
        let sync_manager = sync::SyncManager::new(
            storage,
            config.storage_path.clone(),
            vault_key,
            device_key,
            event_sender.clone(),
        );

        Ok(Self {
            sync_manager,
            event_sender,
            _event_receiver: event_receiver,
        })
    }

    /// Get information about this device
    pub async fn device_info(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: "local-device".to_string(),
            device_name: "Local Device".to_string(),
            last_seen: chrono::Utc::now(),
            is_online: true,
            device_cert: None, // TODO: Load from storage
            is_authorized: true, // Local device is always authorized
        }
    }

    /// Authorize a new device with a device certificate
    pub async fn authorize_device(&mut self, _device_cert: crate::crypto::DeviceCert) -> crate::Result<()> {
        // TODO: Verify device certificate with account key
        // TODO: Store authorized device in storage
        // TODO: Send authorization confirmation
        Ok(())
    }

    /// Revoke authorization for a device
    pub async fn revoke_device(&mut self, _device_id: &str) -> crate::Result<()> {
        // TODO: Remove device from authorized list
        // TODO: Disconnect device if connected
        Ok(())
    }

    /// Check if a device is authorized for this account
    pub async fn is_device_authorized(&self, _device_id: &str) -> crate::Result<bool> {
        // TODO: Check device authorization in storage
        Ok(false) // Default to unauthorized for security
    }

    /// Generate a QR code payload for device linking
    pub fn make_linking_qr(&self) -> QrPayload {
        QrPayload {
            device_id: "local-device".to_string(),
            addresses: Vec::new(),
            onboarding_token: "dummy-token".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
        }
    }

    /// Accept a device link from a QR code payload
    pub async fn accept_link(&self, _payload: QrPayload) -> crate::Result<DeviceInfo> {
        Ok(DeviceInfo {
            device_id: "remote-device".to_string(),
            device_name: "Remote Device".to_string(),
            last_seen: chrono::Utc::now(),
            is_online: true,
            device_cert: None, // TODO: Extract from QR payload
            is_authorized: false, // TODO: Verify device certificate
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
    pub async fn start_network(&self) -> crate::Result<()> {
        // TODO: Implement network startup with proper device authentication
        // This should initialize the network manager and verify device certificates
        Ok(())
    }

    /// Subscribe to events from the account
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<Event> {
        let (_sender, receiver) = mpsc::unbounded_channel();
        // TODO: Implement proper event subscription that forwards events from the main channel
        // For now, return a new channel that won't receive any events
        // This should create a proper subscription that forwards events from the sync manager
        receiver
    }

    /// Force announce current heads to connected peers
    pub async fn force_announce(&self) {
        // TODO: Implement forced announcement to all authorized peers
        // This should trigger a head announcement via the network manager
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
