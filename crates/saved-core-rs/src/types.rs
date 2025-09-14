//! Core types for the SAVED library

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc;
use url::Url;
use crate::storage;
use crate::sync;
use crate::crypto;

/// Unique identifier for a message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub [u8; 32]);

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
}

/// Configuration for the SAVED account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the account storage directory
    pub storage_path: PathBuf,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from("./saved-account"),
            allow_public_relays: false,
            bootstrap_multiaddrs: Vec::new(),
            use_kademlia: false,
            chunk_size: 2 * 1024 * 1024, // 2 MiB
            max_parallel_chunks: 4,
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
    event_sender: mpsc::Sender<Event>,
}

impl AccountHandle {
    /// Create a new account or open an existing one
    pub async fn create_or_open(config: Config) -> crate::Result<Self> {
        // Create storage
        let storage = storage::Storage::open(config.storage_path.clone())?;
        
        // Create event channel
        let (event_sender, _event_receiver) = mpsc::channel();
        
        // Create sync manager
        let vault_key = crypto::generate_vault_key();
        let device_key = crypto::DeviceKey::generate();
        let sync_manager = sync::SyncManager::new(storage, vault_key, device_key, event_sender.clone());
        
        Ok(Self {
            sync_manager,
            event_sender,
        })
    }

    /// Get information about this device
    pub async fn device_info(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: "local-device".to_string(),
            device_name: "Local Device".to_string(),
            last_seen: chrono::Utc::now(),
            is_online: true,
        }
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
        })
    }

    /// Create a new message
    pub async fn create_message(&mut self, body: String, attachments: Vec<PathBuf>) -> crate::Result<MessageId> {
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
        // TODO: Implement network startup
        Ok(())
    }

    /// Subscribe to events from the account
    pub async fn subscribe(&self) -> mpsc::Receiver<Event> {
        let (sender, receiver) = mpsc::channel();
        // TODO: Implement event subscription
        receiver
    }

    /// Force announce current heads to connected peers
    pub async fn force_announce(&self) {
        // TODO: Implement forced announcement
    }

    /// Enable cloud backup (optional feature)
    pub async fn enable_cloud_backup(&self, _endpoint: Url, _token: String) -> crate::Result<()> {
        // TODO: Implement cloud backup
        Ok(())
    }

    /// Disable cloud backup
    pub async fn disable_cloud_backup(&self) -> crate::Result<()> {
        // TODO: Implement cloud backup disable
        Ok(())
    }
}