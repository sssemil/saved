//! Networking layer for SAVED
//!
//! This is a simplified networking implementation that provides the core
//! networking functionality without the full libp2p integration.
//! In a production system, this would be replaced with a full libp2p implementation.

use crate::error::{Error, Result};
use crate::events::{Op, OpHash, EventLog};
use crate::protobuf::*;
use crate::types::{Event, DeviceInfo};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use std::time::Duration;
use tokio::sync::Mutex;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use rand::Rng;

/// Connection state for a peer
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Attempting to connect
    Connecting,
    /// Connected and authenticated
    Connected,
    /// Connection failed
    Failed,
}

/// Network discovery information for a peer
#[derive(Debug, Clone)]
pub struct DiscoveryInfo {
    /// Device ID
    pub device_id: String,
    /// Network addresses where this peer can be reached
    pub addresses: Vec<String>,
    /// Service type (e.g., "_saved._tcp")
    pub service_type: String,
    /// Service name
    pub service_name: String,
    /// Discovery timestamp
    pub discovered_at: DateTime<Utc>,
    /// Last seen via discovery
    pub last_seen: DateTime<Utc>,
    /// Discovery method (mDNS, manual, etc.)
    pub discovery_method: DiscoveryMethod,
}

/// Method used to discover a peer
#[derive(Debug, Clone, PartialEq)]
pub enum DiscoveryMethod {
    /// Discovered via mDNS
    Mdns,
    /// Manually added
    Manual,
    /// Discovered via peer announcement
    PeerAnnouncement,
    /// Discovered via relay
    Relay,
}

/// Enhanced peer information with connection state
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Device information
    pub device_info: DeviceInfo,
    /// Connection state
    pub connection_state: ConnectionState,
    /// Last seen timestamp
    pub last_seen: DateTime<Utc>,
    /// Connection attempts
    pub connection_attempts: u32,
    /// Last connection attempt
    pub last_attempt: Option<DateTime<Utc>>,
    /// Discovery information
    pub discovery_info: Option<DiscoveryInfo>,
}

/// Network manager for SAVED
pub struct NetworkManager {
    /// Event sender for notifying the application
    event_sender: mpsc::UnboundedSender<Event>,
    /// Connected peers with enhanced information
    connected_peers: Arc<Mutex<HashMap<String, PeerInfo>>>,
    /// Discovered peers (not necessarily connected)
    discovered_peers: Arc<Mutex<HashMap<String, DiscoveryInfo>>>,
    /// Event log for sync operations
    event_log: EventLog,
    /// Device key for this node
    device_key: crate::crypto::DeviceKey,
    /// Network addresses we're listening on
    listening_addresses: Arc<Mutex<Vec<String>>>,
    /// Connection timeout duration
    connection_timeout: Duration,
    /// Discovery service name
    service_name: String,
    /// Discovery service type
    service_type: String,
    /// Discovery enabled flag
    discovery_enabled: Arc<Mutex<bool>>,
    /// Discovery interval
    discovery_interval: Duration,
}

impl NetworkManager {
    /// Create a new network manager
    pub async fn new(
        device_key: crate::crypto::DeviceKey,
        event_sender: mpsc::UnboundedSender<Event>,
        event_log: EventLog,
    ) -> Result<Self> {
        Ok(Self {
            event_sender,
            connected_peers: Arc::new(Mutex::new(HashMap::new())),
            discovered_peers: Arc::new(Mutex::new(HashMap::new())),
            event_log,
            device_key,
            listening_addresses: Arc::new(Mutex::new(Vec::new())),
            connection_timeout: Duration::from_secs(30),
            service_name: "SAVED".to_string(),
            service_type: "_saved._tcp".to_string(),
            discovery_enabled: Arc::new(Mutex::new(true)),
            discovery_interval: Duration::from_secs(10),
        })
    }

    /// Start listening on the given addresses
    pub async fn start_listening(&mut self, addresses: Vec<String>) -> Result<()> {
        let mut listening_addrs = self.listening_addresses.lock().await;
        *listening_addrs = addresses.clone();
        drop(listening_addrs);
        
        // In a real implementation, this would start listening on the given addresses
        println!("Network manager started listening on {} addresses", addresses.len());
        
        // Send listening started event
        let _ = self.event_sender.send(Event::SyncProgress { done: 0, total: 100 });
        
        Ok(())
    }

    /// Connect to a peer with proper connection management
    pub async fn connect_to_peer(&mut self, device_id: String, addresses: Vec<String>) -> Result<()> {
        let mut peers = self.connected_peers.lock().await;
        
        // Check if peer is already connected
        if let Some(peer_info) = peers.get(&device_id) {
            match peer_info.connection_state {
                ConnectionState::Connected => {
                    return Ok(()); // Already connected
                }
                ConnectionState::Connecting => {
                    return Err(Error::Network("Connection already in progress".to_string()));
                }
                _ => {} // Continue with connection attempt
            }
        }
        
        // Create peer info with connecting state
        let device_info = DeviceInfo {
            device_id: device_id.clone(),
            device_name: format!("Device {}", &device_id[..8]),
            last_seen: Utc::now(),
            is_online: false,
            device_cert: None,
            is_authorized: false,
        };
        
        // Create discovery info
        let discovery_info = DiscoveryInfo {
            device_id: device_id.clone(),
            addresses: addresses.clone(),
            service_type: self.service_type.clone(),
            service_name: format!("{}-{}", self.service_name, &device_id[..8]),
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            discovery_method: DiscoveryMethod::Manual,
        };

        let peer_info = PeerInfo {
            device_info: device_info.clone(),
            connection_state: ConnectionState::Connecting,
            last_seen: Utc::now(),
            connection_attempts: 1,
            last_attempt: Some(Utc::now()),
            discovery_info: Some(discovery_info.clone()),
        };
        
        peers.insert(device_id.clone(), peer_info);
        drop(peers);
        
        // Add to discovered peers
        let mut discovered = self.discovered_peers.lock().await;
        discovered.insert(device_id.clone(), discovery_info);
        drop(discovered);
        
        // Simulate connection attempt
        let connection_result = self.simulate_connection_attempt(&device_id, addresses).await;
        
        // Update connection state based on result
        let mut peers = self.connected_peers.lock().await;
        if let Some(peer_info) = peers.get_mut(&device_id) {
            match connection_result {
                Ok(_) => {
                    peer_info.connection_state = ConnectionState::Connected;
                    peer_info.device_info.is_online = true;
                    peer_info.last_seen = Utc::now();
                    
                    // Send connection success event
                    let _ = self.event_sender.send(Event::Connected(peer_info.device_info.clone()));
                }
                Err(e) => {
                    peer_info.connection_state = ConnectionState::Failed;
                    peer_info.connection_attempts += 1;
                    
                    // Send connection failure event
                    let _ = self.event_sender.send(Event::Disconnected(device_id.clone()));
                    
                    return Err(e);
                }
            }
        }
        
        Ok(())
    }

    /// Simulate a connection attempt (placeholder for real networking)
    async fn simulate_connection_attempt(&self, device_id: &str, _addresses: Vec<String>) -> Result<()> {
        // Simulate connection delay
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Simulate connection success/failure based on device_id
        if device_id.contains("fail") {
            Err(Error::Network("Simulated connection failure".to_string()))
        } else {
            Ok(())
        }
    }

    /// Run the network event loop (placeholder)
    pub async fn run(&mut self) -> Result<()> {
        // In a real implementation, this would run the network event loop
        // For now, we'll just wait indefinitely
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Get connected peers
    pub async fn get_connected_peers(&self) -> HashMap<String, DeviceInfo> {
        let peers = self.connected_peers.lock().await;
        peers.iter()
            .filter(|(_, peer_info)| peer_info.connection_state == ConnectionState::Connected)
            .map(|(device_id, peer_info)| (device_id.clone(), peer_info.device_info.clone()))
            .collect()
    }

    /// Get all peers (including disconnected ones)
    pub async fn get_all_peers(&self) -> HashMap<String, PeerInfo> {
        let peers = self.connected_peers.lock().await;
        peers.clone()
    }

    /// Disconnect a specific peer with proper cleanup
    pub async fn disconnect_peer(&mut self, device_id: String) -> Result<()> {
        let mut peers = self.connected_peers.lock().await;
        
        if let Some(peer_info) = peers.get_mut(&device_id) {
            // Update connection state
            peer_info.connection_state = ConnectionState::Disconnected;
            peer_info.device_info.is_online = false;
            peer_info.last_seen = Utc::now();
            
            // Send disconnection event
            let _ = self.event_sender.send(Event::Disconnected(device_id.clone()));
            
            // Remove from peers map after a delay (for cleanup)
            tokio::spawn({
                let peers = self.connected_peers.clone();
                let device_id = device_id.clone();
                async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    let mut peers = peers.lock().await;
                    peers.remove(&device_id);
                }
            });
        }
        
        Ok(())
    }

    /// Force disconnect and remove a peer immediately
    pub async fn force_disconnect_peer(&mut self, device_id: String) -> Result<()> {
        let mut peers = self.connected_peers.lock().await;
        
        if peers.contains_key(&device_id) {
            // Send disconnection event
            let _ = self.event_sender.send(Event::Disconnected(device_id.clone()));
            
            // Remove immediately
            peers.remove(&device_id);
        }
        
        Ok(())
    }

    /// Check if a peer is connected
    pub async fn is_peer_connected(&self, device_id: &str) -> bool {
        let peers = self.connected_peers.lock().await;
        peers.get(device_id)
            .map(|peer_info| peer_info.connection_state == ConnectionState::Connected)
            .unwrap_or(false)
    }

    /// Get connection state for a peer
    pub async fn get_peer_connection_state(&self, device_id: &str) -> Option<ConnectionState> {
        let peers = self.connected_peers.lock().await;
        peers.get(device_id).map(|peer_info| peer_info.connection_state.clone())
    }

    // ===== NETWORK DISCOVERY METHODS =====

    /// Start network discovery
    pub async fn start_discovery(&mut self) -> Result<()> {
        let mut enabled = self.discovery_enabled.lock().await;
        *enabled = true;
        drop(enabled);
        
        // Start discovery background task
        let discovery_interval = self.discovery_interval;
        let discovered_peers = self.discovered_peers.clone();
        let event_sender = self.event_sender.clone();
        let service_type = self.service_type.clone();
        
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(discovery_interval).await;
                
                // Check if discovery is still enabled
                // In a real implementation, this would be checked via a shared flag
                
                // Simulate mDNS discovery
                Self::simulate_mdns_discovery(&discovered_peers, &event_sender, &service_type).await;
            }
        });
        
        Ok(())
    }

    /// Stop network discovery
    pub async fn stop_discovery(&mut self) -> Result<()> {
        let mut enabled = self.discovery_enabled.lock().await;
        *enabled = false;
        Ok(())
    }

    /// Simulate mDNS discovery (placeholder for real mDNS implementation)
    async fn simulate_mdns_discovery(
        discovered_peers: &Arc<Mutex<HashMap<String, DiscoveryInfo>>>,
        event_sender: &mpsc::UnboundedSender<Event>,
        service_type: &str,
    ) {
        // Use a simple random check to avoid Send issues with ThreadRng
        let random_value = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() % 100;
        
        // Simulate discovering a random peer occasionally (30% chance)
        if random_value < 30 {
            let device_id = format!("discovered-{:04x}", random_value as u16);
            let addresses = vec![
                format!("/ip4/192.168.1.{}", (random_value % 254) + 1),
                format!("/ip4/10.0.0.{}", ((random_value * 2) % 254) + 1),
            ];
            
            let discovery_info = DiscoveryInfo {
                device_id: device_id.clone(),
                addresses,
                service_type: service_type.to_string(),
                service_name: format!("SAVED-{}", &device_id[..8]),
                discovered_at: Utc::now(),
                last_seen: Utc::now(),
                discovery_method: DiscoveryMethod::Mdns,
            };
            
            // Add to discovered peers
            let mut discovered = discovered_peers.lock().await;
            discovered.insert(device_id.clone(), discovery_info.clone());
            drop(discovered);
            
            // Send discovery event
            let _ = event_sender.send(Event::SyncProgress { done: 1, total: 100 });
        }
    }

    /// Get all discovered peers
    pub async fn get_discovered_peers(&self) -> HashMap<String, DiscoveryInfo> {
        let discovered = self.discovered_peers.lock().await;
        discovered.clone()
    }

    /// Get discovered peers by discovery method
    pub async fn get_discovered_peers_by_method(&self, method: DiscoveryMethod) -> HashMap<String, DiscoveryInfo> {
        let discovered = self.discovered_peers.lock().await;
        discovered.iter()
            .filter(|(_, info)| info.discovery_method == method)
            .map(|(device_id, info)| (device_id.clone(), info.clone()))
            .collect()
    }

    /// Manually add a discovered peer
    pub async fn add_discovered_peer(&mut self, device_id: String, addresses: Vec<String>, method: DiscoveryMethod) -> Result<()> {
        let discovery_info = DiscoveryInfo {
            device_id: device_id.clone(),
            addresses,
            service_type: self.service_type.clone(),
            service_name: format!("{}-{}", self.service_name, &device_id[..8]),
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            discovery_method: method,
        };
        
        let mut discovered = self.discovered_peers.lock().await;
        discovered.insert(device_id.clone(), discovery_info);
        drop(discovered);
        
        // Send discovery event
        let _ = self.event_sender.send(Event::SyncProgress { done: 1, total: 100 });
        
        Ok(())
    }

    /// Remove a discovered peer
    pub async fn remove_discovered_peer(&mut self, device_id: &str) -> Result<()> {
        let mut discovered = self.discovered_peers.lock().await;
        discovered.remove(device_id);
        Ok(())
    }

    /// Scan for peers on the local network
    pub async fn scan_local_network(&mut self) -> Result<Vec<DiscoveryInfo>> {
        let mut discovered_peers = Vec::new();
        let mut rng = rand::thread_rng();
        
        // Simulate scanning common local network ranges
        let network_ranges = vec![
            "192.168.1.0/24",
            "192.168.0.0/24", 
            "10.0.0.0/24",
            "172.16.0.0/24",
        ];
        
        for range in network_ranges {
            // Simulate finding 0-2 peers per range
            let peer_count = rng.gen_range(0..3);
            for _ in 0..peer_count {
                let device_id = format!("scanned-{:04x}", rng.gen::<u16>());
                let ip = match range {
                    "192.168.1.0/24" => format!("192.168.1.{}", rng.gen_range(1..255)),
                    "192.168.0.0/24" => format!("192.168.0.{}", rng.gen_range(1..255)),
                    "10.0.0.0/24" => format!("10.0.0.{}", rng.gen_range(1..255)),
                    "172.16.0.0/24" => format!("172.16.0.{}", rng.gen_range(1..255)),
                    _ => continue,
                };
                
                let discovery_info = DiscoveryInfo {
                    device_id: device_id.clone(),
                    addresses: vec![format!("/ip4/{}/tcp/8080", ip)],
                    service_type: self.service_type.clone(),
                    service_name: format!("SAVED-{}", &device_id[..8]),
                    discovered_at: Utc::now(),
                    last_seen: Utc::now(),
                    discovery_method: DiscoveryMethod::Manual, // Manual scan
                };
                
                discovered_peers.push(discovery_info.clone());
                
                // Add to discovered peers
                let mut discovered = self.discovered_peers.lock().await;
                discovered.insert(device_id, discovery_info);
                drop(discovered);
            }
        }
        
        Ok(discovered_peers)
    }

    /// Get peers discovered via mDNS
    pub async fn get_mdns_peers(&self) -> HashMap<String, DiscoveryInfo> {
        self.get_discovered_peers_by_method(DiscoveryMethod::Mdns).await
    }

    /// Get peers discovered via manual scan
    pub async fn get_manually_discovered_peers(&self) -> HashMap<String, DiscoveryInfo> {
        self.get_discovered_peers_by_method(DiscoveryMethod::Manual).await
    }

    /// Check if discovery is enabled
    pub async fn is_discovery_enabled(&self) -> bool {
        let enabled = self.discovery_enabled.lock().await;
        *enabled
    }

    /// Set discovery interval
    pub fn set_discovery_interval(&mut self, interval: Duration) {
        self.discovery_interval = interval;
    }

    /// Check if a peer is authorized
    pub async fn is_peer_authorized(&self, device_id: &str) -> Result<bool> {
        // In a real implementation, this would check against the storage
        // For now, we'll just return true for connected peers
        let peers = self.connected_peers.lock().await;
        Ok(peers.contains_key(device_id))
    }

    /// Handle chunk message (placeholder)
    pub async fn handle_chunks_message(&mut self, _message: Vec<u8>) -> Result<()> {
        // In a real implementation, this would handle chunk requests/responses
        Ok(())
    }

    /// Handle operations message (placeholder)
    pub async fn handle_ops_message(&mut self, _message: Vec<u8>) -> Result<()> {
        // In a real implementation, this would handle operation requests/responses
        Ok(())
    }

    /// Encrypt operation for transmission (placeholder)
    pub async fn encrypt_operation_for_transmission(&self, _op: &Op) -> Result<OpEnvelope> {
        // In a real implementation, this would encrypt the operation
        // For now, we'll return a placeholder envelope
        Ok(OpEnvelope {
            header: Some(OpHeader {
                op_id: vec![0; 40],
                lamport: 0,
                parents: vec![],
                signer: self.device_key.public_key_bytes().to_vec(),
                sig: vec![0; 64],
                timestamp: chrono::Utc::now().timestamp(),
            }),
            ciphertext: vec![0; 100], // Placeholder ciphertext
        })
    }

    /// Decrypt and verify operation (placeholder)
    pub async fn decrypt_and_verify_operation(&self, _envelope: OpEnvelope) -> Result<Op> {
        // In a real implementation, this would decrypt and verify the operation
        // For now, we'll return a placeholder operation
        Err(Error::Network("Operation decryption not implemented".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DeviceKey;

    #[tokio::test]
    async fn test_network_manager_creation() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let network_manager = NetworkManager::new(device_key, event_sender, event_log).await;
        assert!(network_manager.is_ok());
    }

    #[tokio::test]
    async fn test_network_manager_listening() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        let addresses = vec!["/ip4/127.0.0.1/tcp/0".to_string()];
        
        let result = network_manager.start_listening(addresses).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_peer_connection() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Test connecting to a peer
        let result = network_manager.connect_to_peer("test-peer".to_string(), vec!["/ip4/127.0.0.1/tcp/8080".to_string()]).await;
        assert!(result.is_ok());
        
        // Check that peer is connected
        let peers = network_manager.get_connected_peers().await;
        assert!(peers.contains_key("test-peer"));
        
        // Check connection state
        let is_connected = network_manager.is_peer_connected("test-peer").await;
        assert!(is_connected);
        
        // Test disconnecting peer
        let result = network_manager.disconnect_peer("test-peer".to_string()).await;
        assert!(result.is_ok());
        
        // Check that peer is disconnected
        let is_connected = network_manager.is_peer_connected("test-peer").await;
        assert!(!is_connected);
    }

    #[tokio::test]
    async fn test_connection_failure() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Test connecting to a peer that will fail
        let result = network_manager.connect_to_peer("fail-peer".to_string(), vec!["/ip4/127.0.0.1/tcp/8080".to_string()]).await;
        assert!(result.is_err());
        
        // Check that peer is not connected
        let is_connected = network_manager.is_peer_connected("fail-peer").await;
        assert!(!is_connected);
        
        // Check connection state
        let state = network_manager.get_peer_connection_state("fail-peer").await;
        assert!(matches!(state, Some(ConnectionState::Failed)));
    }

    #[tokio::test]
    async fn test_connection_states() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Initially no connection state
        let state = network_manager.get_peer_connection_state("nonexistent-peer").await;
        assert!(state.is_none());
        
        // Connect to a peer
        let result = network_manager.connect_to_peer("test-peer".to_string(), vec!["/ip4/127.0.0.1/tcp/8080".to_string()]).await;
        assert!(result.is_ok());
        
        // Check connected state
        let state = network_manager.get_peer_connection_state("test-peer").await;
        assert!(matches!(state, Some(ConnectionState::Connected)));
        
        // Disconnect peer
        let result = network_manager.disconnect_peer("test-peer".to_string()).await;
        assert!(result.is_ok());
        
        // Check disconnected state
        let state = network_manager.get_peer_connection_state("test-peer").await;
        assert!(matches!(state, Some(ConnectionState::Disconnected)));
    }

    #[tokio::test]
    async fn test_network_discovery() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Initially no discovered peers
        let discovered = network_manager.get_discovered_peers().await;
        assert!(discovered.is_empty());
        
        // Start discovery
        let result = network_manager.start_discovery().await;
        assert!(result.is_ok());
        
        // Check discovery is enabled
        let enabled = network_manager.is_discovery_enabled().await;
        assert!(enabled);
        
        // Stop discovery
        let result = network_manager.stop_discovery().await;
        assert!(result.is_ok());
        
        let enabled = network_manager.is_discovery_enabled().await;
        assert!(!enabled);
    }

    #[tokio::test]
    async fn test_manual_peer_discovery() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Manually add a discovered peer
        let device_id = "manual-peer".to_string();
        let addresses = vec!["/ip4/192.168.1.100/tcp/8080".to_string()];
        let result = network_manager.add_discovered_peer(device_id.clone(), addresses, DiscoveryMethod::Manual).await;
        assert!(result.is_ok());
        
        // Check peer was added
        let discovered = network_manager.get_discovered_peers().await;
        assert!(discovered.contains_key(&device_id));
        
        // Check discovery method
        let manual_peers = network_manager.get_manually_discovered_peers().await;
        assert!(manual_peers.contains_key(&device_id));
        
        // Remove discovered peer
        let result = network_manager.remove_discovered_peer(&device_id).await;
        assert!(result.is_ok());
        
        // Check peer was removed
        let discovered = network_manager.get_discovered_peers().await;
        assert!(!discovered.contains_key(&device_id));
    }

    #[tokio::test]
    async fn test_network_scanning() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Scan local network
        let discovered_peers = network_manager.scan_local_network().await.unwrap();
        
        // Check that some peers were discovered (simulated)
        // Note: This test might find 0 peers due to randomness, which is fine
        assert!(discovered_peers.len() <= 8); // Max 2 peers per 4 ranges
        
        // Check that discovered peers were added to the manager
        let all_discovered = network_manager.get_discovered_peers().await;
        assert!(all_discovered.len() >= discovered_peers.len());
    }

    #[tokio::test]
    async fn test_discovery_methods() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Add peers with different discovery methods
        network_manager.add_discovered_peer(
            "mdns-peer".to_string(),
            vec!["/ip4/192.168.1.101/tcp/8080".to_string()],
            DiscoveryMethod::Mdns
        ).await.unwrap();
        
        network_manager.add_discovered_peer(
            "manual-peer".to_string(),
            vec!["/ip4/192.168.1.102/tcp/8080".to_string()],
            DiscoveryMethod::Manual
        ).await.unwrap();
        
        network_manager.add_discovered_peer(
            "relay-peer".to_string(),
            vec!["/ip4/192.168.1.103/tcp/8080".to_string()],
            DiscoveryMethod::Relay
        ).await.unwrap();
        
        // Check mDNS peers
        let mdns_peers = network_manager.get_mdns_peers().await;
        assert!(mdns_peers.contains_key("mdns-peer"));
        assert!(!mdns_peers.contains_key("manual-peer"));
        
        // Check manual peers
        let manual_peers = network_manager.get_manually_discovered_peers().await;
        assert!(manual_peers.contains_key("manual-peer"));
        assert!(!manual_peers.contains_key("mdns-peer"));
        
        // Check relay peers
        let relay_peers = network_manager.get_discovered_peers_by_method(DiscoveryMethod::Relay).await;
        assert!(relay_peers.contains_key("relay-peer"));
        assert!(!relay_peers.contains_key("manual-peer"));
    }

    #[tokio::test]
    async fn test_discovery_interval() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Set custom discovery interval
        let custom_interval = Duration::from_secs(5);
        network_manager.set_discovery_interval(custom_interval);
        
        // The interval is set (we can't easily test the actual timing without more complex setup)
        // This test mainly ensures the method doesn't panic
        assert!(true);
    }
}