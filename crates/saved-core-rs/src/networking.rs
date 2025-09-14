//! Networking layer for SAVED
//!
//! This is a simplified networking implementation that provides the core
//! networking functionality without the full libp2p integration.
//! In a production system, this would be replaced with a full libp2p implementation.

use crate::error::{Error, Result};
use crate::events::{Op, OpHash, EventLog};
use crate::protobuf::*;
use crate::types::{Event, DeviceInfo};
use std::collections::{HashMap, HashSet, VecDeque};
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

/// Peer health status
#[derive(Debug, Clone, PartialEq)]
pub enum PeerHealth {
    /// Healthy and responsive
    Healthy,
    /// Experiencing issues but still connected
    Degraded,
    /// Unresponsive or frequently disconnecting
    Unhealthy,
    /// Unknown health status
    Unknown,
}

/// Peer statistics for monitoring
#[derive(Debug, Clone)]
pub struct PeerStats {
    /// Total connection attempts
    pub total_connection_attempts: u32,
    /// Successful connections
    pub successful_connections: u32,
    /// Failed connections
    pub failed_connections: u32,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Last successful connection
    pub last_successful_connection: Option<DateTime<Utc>>,
    /// Last failed connection
    pub last_failed_connection: Option<DateTime<Utc>>,
    /// Average connection duration
    pub avg_connection_duration: Duration,
    /// Total connection time
    pub total_connection_time: Duration,
    /// Number of disconnections
    pub disconnection_count: u32,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
}

/// Peer group for organizing peers
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PeerGroup {
    /// Local network peers
    Local,
    /// Internet peers
    Internet,
    /// Relay peers
    Relay,
    /// Trusted peers
    Trusted,
    /// Blocked peers
    Blocked,
    /// Custom group
    Custom(String),
}

/// Peer management configuration
#[derive(Debug, Clone)]
pub struct PeerConfig {
    /// Maximum connection attempts before marking as failed
    pub max_connection_attempts: u32,
    /// Connection timeout duration
    pub connection_timeout: Duration,
    /// Health check interval
    pub health_check_interval: Duration,
    /// Maximum idle time before disconnection
    pub max_idle_time: Duration,
    /// Peer group
    pub group: PeerGroup,
    /// Auto-reconnect enabled
    pub auto_reconnect: bool,
    /// Priority level (higher = more important)
    pub priority: u8,
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
    /// Peer health status
    pub health: PeerHealth,
    /// Peer statistics
    pub stats: PeerStats,
    /// Peer configuration
    pub config: PeerConfig,
    /// Connection start time (if connected)
    pub connection_start_time: Option<DateTime<Utc>>,
    /// Last health check
    pub last_health_check: Option<DateTime<Utc>>,
    /// Peer tags for categorization
    pub tags: HashSet<String>,
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
    /// Peer management settings
    peer_management_enabled: Arc<Mutex<bool>>,
    /// Health check interval
    health_check_interval: Duration,
    /// Peer statistics history
    peer_stats_history: Arc<Mutex<HashMap<String, VecDeque<PeerStats>>>>,
    /// Peer groups
    peer_groups: Arc<Mutex<HashMap<PeerGroup, HashSet<String>>>>,
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
            peer_management_enabled: Arc::new(Mutex::new(true)),
            health_check_interval: Duration::from_secs(60),
            peer_stats_history: Arc::new(Mutex::new(HashMap::new())),
            peer_groups: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Create default peer configuration
    fn create_default_peer_config() -> PeerConfig {
        PeerConfig {
            max_connection_attempts: 5,
            connection_timeout: Duration::from_secs(30),
            health_check_interval: Duration::from_secs(60),
            max_idle_time: Duration::from_secs(300), // 5 minutes
            group: PeerGroup::Local,
            auto_reconnect: true,
            priority: 5, // Medium priority
        }
    }

    /// Create default peer statistics
    fn create_default_peer_stats() -> PeerStats {
        PeerStats {
            total_connection_attempts: 0,
            successful_connections: 0,
            failed_connections: 0,
            bytes_sent: 0,
            bytes_received: 0,
            last_successful_connection: None,
            last_failed_connection: None,
            avg_connection_duration: Duration::from_secs(0),
            total_connection_time: Duration::from_secs(0),
            disconnection_count: 0,
            last_activity: Utc::now(),
        }
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
            health: PeerHealth::Unknown,
            stats: Self::create_default_peer_stats(),
            config: Self::create_default_peer_config(),
            connection_start_time: None,
            last_health_check: None,
            tags: HashSet::new(),
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

    // ===== PEER MANAGEMENT METHODS =====

    /// Start peer management (health checks, statistics, etc.)
    pub async fn start_peer_management(&mut self) -> Result<()> {
        let mut enabled = self.peer_management_enabled.lock().await;
        *enabled = true;
        drop(enabled);
        
        // Start health check background task
        let health_check_interval = self.health_check_interval;
        let connected_peers = self.connected_peers.clone();
        let event_sender = self.event_sender.clone();
        
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(health_check_interval).await;
                
                // Perform health checks on all connected peers
                Self::perform_health_checks(&connected_peers, &event_sender).await;
            }
        });
        
        Ok(())
    }

    /// Stop peer management
    pub async fn stop_peer_management(&mut self) -> Result<()> {
        let mut enabled = self.peer_management_enabled.lock().await;
        *enabled = false;
        Ok(())
    }

    /// Perform health checks on all connected peers
    async fn perform_health_checks(
        connected_peers: &Arc<Mutex<HashMap<String, PeerInfo>>>,
        _event_sender: &mpsc::UnboundedSender<Event>,
    ) {
        let mut peers = connected_peers.lock().await;
        let now = Utc::now();
        
        for (_device_id, peer_info) in peers.iter_mut() {
            if peer_info.connection_state == ConnectionState::Connected {
                // Update last health check
                peer_info.last_health_check = Some(now);
                
                // Simple health check: if last activity was too long ago, mark as degraded
                let time_since_activity = now.signed_duration_since(peer_info.stats.last_activity);
                if time_since_activity.num_seconds() > 300 { // 5 minutes
                    peer_info.health = PeerHealth::Degraded;
                } else {
                    peer_info.health = PeerHealth::Healthy;
                }
                
                // Update last seen
                peer_info.last_seen = now;
            }
        }
    }

    /// Get peer statistics
    pub async fn get_peer_stats(&self, device_id: &str) -> Option<PeerStats> {
        let peers = self.connected_peers.lock().await;
        peers.get(device_id).map(|peer_info| peer_info.stats.clone())
    }

    /// Update peer statistics
    pub async fn update_peer_stats(&mut self, device_id: &str, stats: PeerStats) -> Result<()> {
        let mut peers = self.connected_peers.lock().await;
        if let Some(peer_info) = peers.get_mut(device_id) {
            peer_info.stats = stats.clone();
            
            // Add to statistics history
            let mut history = self.peer_stats_history.lock().await;
            let history_entry = history.entry(device_id.to_string()).or_insert_with(VecDeque::new);
            history_entry.push_back(stats);
            
            // Keep only last 100 entries
            if history_entry.len() > 100 {
                history_entry.pop_front();
            }
        }
        Ok(())
    }

    /// Get peer health status
    pub async fn get_peer_health(&self, device_id: &str) -> Option<PeerHealth> {
        let peers = self.connected_peers.lock().await;
        peers.get(device_id).map(|peer_info| peer_info.health.clone())
    }

    /// Set peer health status
    pub async fn set_peer_health(&mut self, device_id: &str, health: PeerHealth) -> Result<()> {
        let mut peers = self.connected_peers.lock().await;
        if let Some(peer_info) = peers.get_mut(device_id) {
            peer_info.health = health;
        }
        Ok(())
    }

    /// Get peer configuration
    pub async fn get_peer_config(&self, device_id: &str) -> Option<PeerConfig> {
        let peers = self.connected_peers.lock().await;
        peers.get(device_id).map(|peer_info| peer_info.config.clone())
    }

    /// Update peer configuration
    pub async fn update_peer_config(&mut self, device_id: &str, config: PeerConfig) -> Result<()> {
        let mut peers = self.connected_peers.lock().await;
        if let Some(peer_info) = peers.get_mut(device_id) {
            peer_info.config = config.clone();
            
            // Update peer groups
            let mut groups = self.peer_groups.lock().await;
            
            // Remove from old group
            for (group, peer_set) in groups.iter_mut() {
                peer_set.remove(device_id);
            }
            
            // Add to new group
            groups.entry(config.group.clone()).or_insert_with(HashSet::new).insert(device_id.to_string());
        }
        Ok(())
    }

    /// Add tag to peer
    pub async fn add_peer_tag(&mut self, device_id: &str, tag: String) -> Result<()> {
        let mut peers = self.connected_peers.lock().await;
        if let Some(peer_info) = peers.get_mut(device_id) {
            peer_info.tags.insert(tag);
        }
        Ok(())
    }

    /// Remove tag from peer
    pub async fn remove_peer_tag(&mut self, device_id: &str, tag: &str) -> Result<()> {
        let mut peers = self.connected_peers.lock().await;
        if let Some(peer_info) = peers.get_mut(device_id) {
            peer_info.tags.remove(tag);
        }
        Ok(())
    }

    /// Get peers by tag
    pub async fn get_peers_by_tag(&self, tag: &str) -> Vec<String> {
        let peers = self.connected_peers.lock().await;
        peers.iter()
            .filter(|(_, peer_info)| peer_info.tags.contains(tag))
            .map(|(device_id, _)| device_id.clone())
            .collect()
    }

    /// Get peers by group
    pub async fn get_peers_by_group(&self, group: &PeerGroup) -> Vec<String> {
        let groups = self.peer_groups.lock().await;
        groups.get(group).map(|peer_set| peer_set.iter().cloned().collect()).unwrap_or_default()
    }

    /// Get all peer groups
    pub async fn get_peer_groups(&self) -> HashMap<PeerGroup, Vec<String>> {
        let groups = self.peer_groups.lock().await;
        groups.iter()
            .map(|(group, peer_set)| (group.clone(), peer_set.iter().cloned().collect()))
            .collect()
    }

    /// Get peer statistics history
    pub async fn get_peer_stats_history(&self, device_id: &str) -> Vec<PeerStats> {
        let history = self.peer_stats_history.lock().await;
        history.get(device_id).map(|deque| deque.iter().cloned().collect()).unwrap_or_default()
    }

    /// Get all peer statistics
    pub async fn get_all_peer_stats(&self) -> HashMap<String, PeerStats> {
        let peers = self.connected_peers.lock().await;
        peers.iter()
            .map(|(device_id, peer_info)| (device_id.clone(), peer_info.stats.clone()))
            .collect()
    }

    /// Get peers by health status
    pub async fn get_peers_by_health(&self, health: &PeerHealth) -> Vec<String> {
        let peers = self.connected_peers.lock().await;
        peers.iter()
            .filter(|(_, peer_info)| peer_info.health == *health)
            .map(|(device_id, _)| device_id.clone())
            .collect()
    }

    /// Get peer management summary
    pub async fn get_peer_management_summary(&self) -> HashMap<String, u32> {
        let peers = self.connected_peers.lock().await;
        let mut summary = HashMap::new();
        
        summary.insert("total_peers".to_string(), peers.len() as u32);
        summary.insert("connected_peers".to_string(), 
            peers.values().filter(|p| p.connection_state == ConnectionState::Connected).count() as u32);
        summary.insert("healthy_peers".to_string(), 
            peers.values().filter(|p| p.health == PeerHealth::Healthy).count() as u32);
        summary.insert("unhealthy_peers".to_string(), 
            peers.values().filter(|p| p.health == PeerHealth::Unhealthy).count() as u32);
        
        summary
    }

    /// Check if peer management is enabled
    pub async fn is_peer_management_enabled(&self) -> bool {
        let enabled = self.peer_management_enabled.lock().await;
        *enabled
    }

    /// Set health check interval
    pub fn set_health_check_interval(&mut self, interval: Duration) {
        self.health_check_interval = interval;
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

    #[tokio::test]
    async fn test_peer_management() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Initially peer management should be enabled
        let enabled = network_manager.is_peer_management_enabled().await;
        assert!(enabled);
        
        // Start peer management
        let result = network_manager.start_peer_management().await;
        assert!(result.is_ok());
        
        // Stop peer management
        let result = network_manager.stop_peer_management().await;
        assert!(result.is_ok());
        
        let enabled = network_manager.is_peer_management_enabled().await;
        assert!(!enabled);
    }

    #[tokio::test]
    async fn test_peer_health_management() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Connect to a peer
        let device_id = "health-test-peer".to_string();
        let result = network_manager.connect_to_peer(device_id.clone(), vec!["/ip4/127.0.0.1/tcp/8080".to_string()]).await;
        assert!(result.is_ok());
        
        // Check initial health (should be Unknown)
        let health = network_manager.get_peer_health(&device_id).await;
        assert!(matches!(health, Some(PeerHealth::Unknown)));
        
        // Set health to Healthy
        let result = network_manager.set_peer_health(&device_id, PeerHealth::Healthy).await;
        assert!(result.is_ok());
        
        let health = network_manager.get_peer_health(&device_id).await;
        assert!(matches!(health, Some(PeerHealth::Healthy)));
        
        // Get peers by health
        let healthy_peers = network_manager.get_peers_by_health(&PeerHealth::Healthy).await;
        assert!(healthy_peers.contains(&device_id));
    }

    #[tokio::test]
    async fn test_peer_statistics() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Connect to a peer
        let device_id = "stats-test-peer".to_string();
        let result = network_manager.connect_to_peer(device_id.clone(), vec!["/ip4/127.0.0.1/tcp/8080".to_string()]).await;
        assert!(result.is_ok());
        
        // Get initial stats
        let stats = network_manager.get_peer_stats(&device_id).await;
        assert!(stats.is_some());
        let initial_stats = stats.unwrap();
        assert_eq!(initial_stats.total_connection_attempts, 0);
        
        // Update stats
        let mut new_stats = initial_stats.clone();
        new_stats.total_connection_attempts = 5;
        new_stats.bytes_sent = 1024;
        new_stats.bytes_received = 2048;
        
        let result = network_manager.update_peer_stats(&device_id, new_stats.clone()).await;
        assert!(result.is_ok());
        
        // Verify stats were updated
        let updated_stats = network_manager.get_peer_stats(&device_id).await.unwrap();
        assert_eq!(updated_stats.total_connection_attempts, 5);
        assert_eq!(updated_stats.bytes_sent, 1024);
        assert_eq!(updated_stats.bytes_received, 2048);
        
        // Check stats history
        let history = network_manager.get_peer_stats_history(&device_id).await;
        assert!(history.len() >= 1);
    }

    #[tokio::test]
    async fn test_peer_groups() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Connect to a peer
        let device_id = "group-test-peer".to_string();
        let result = network_manager.connect_to_peer(device_id.clone(), vec!["/ip4/127.0.0.1/tcp/8080".to_string()]).await;
        assert!(result.is_ok());
        
        // Get initial config (should be Local group)
        let config = network_manager.get_peer_config(&device_id).await.unwrap();
        assert!(matches!(config.group, PeerGroup::Local));
        
        // Update config to Internet group
        let mut new_config = config.clone();
        new_config.group = PeerGroup::Internet;
        new_config.priority = 10;
        
        let result = network_manager.update_peer_config(&device_id, new_config).await;
        assert!(result.is_ok());
        
        // Check peer is in Internet group
        let internet_peers = network_manager.get_peers_by_group(&PeerGroup::Internet).await;
        assert!(internet_peers.contains(&device_id));
        
        // Check peer is not in Local group
        let local_peers = network_manager.get_peers_by_group(&PeerGroup::Local).await;
        assert!(!local_peers.contains(&device_id));
        
        // Get all groups
        let groups = network_manager.get_peer_groups().await;
        assert!(groups.contains_key(&PeerGroup::Internet));
    }

    #[tokio::test]
    async fn test_peer_tags() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Connect to a peer
        let device_id = "tag-test-peer".to_string();
        let result = network_manager.connect_to_peer(device_id.clone(), vec!["/ip4/127.0.0.1/tcp/8080".to_string()]).await;
        assert!(result.is_ok());
        
        // Add tags
        let result = network_manager.add_peer_tag(&device_id, "important".to_string()).await;
        assert!(result.is_ok());
        
        let result = network_manager.add_peer_tag(&device_id, "mobile".to_string()).await;
        assert!(result.is_ok());
        
        // Get peers by tag
        let important_peers = network_manager.get_peers_by_tag("important").await;
        assert!(important_peers.contains(&device_id));
        
        let mobile_peers = network_manager.get_peers_by_tag("mobile").await;
        assert!(mobile_peers.contains(&device_id));
        
        // Remove tag
        let result = network_manager.remove_peer_tag(&device_id, "mobile").await;
        assert!(result.is_ok());
        
        // Check tag was removed
        let mobile_peers = network_manager.get_peers_by_tag("mobile").await;
        assert!(!mobile_peers.contains(&device_id));
        
        // Important tag should still be there
        let important_peers = network_manager.get_peers_by_tag("important").await;
        assert!(important_peers.contains(&device_id));
    }

    #[tokio::test]
    async fn test_peer_management_summary() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let event_log = EventLog::new();
        
        let mut network_manager = NetworkManager::new(device_key, event_sender, event_log).await.unwrap();
        
        // Initially no peers
        let summary = network_manager.get_peer_management_summary().await;
        assert_eq!(summary.get("total_peers").unwrap(), &0);
        assert_eq!(summary.get("connected_peers").unwrap(), &0);
        
        // Connect to a peer
        let device_id = "summary-test-peer".to_string();
        let result = network_manager.connect_to_peer(device_id.clone(), vec!["/ip4/127.0.0.1/tcp/8080".to_string()]).await;
        assert!(result.is_ok());
        
        // Set peer as healthy
        let result = network_manager.set_peer_health(&device_id, PeerHealth::Healthy).await;
        assert!(result.is_ok());
        
        // Check summary
        let summary = network_manager.get_peer_management_summary().await;
        assert_eq!(summary.get("total_peers").unwrap(), &1);
        assert_eq!(summary.get("connected_peers").unwrap(), &1);
        assert_eq!(summary.get("healthy_peers").unwrap(), &1);
        assert_eq!(summary.get("unhealthy_peers").unwrap(), &0);
    }
}