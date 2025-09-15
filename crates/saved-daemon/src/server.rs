//! Daemon server for handling control requests

use anyhow::Result;
use saved_core::AccountHandle;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonRequest {
    Status,
    DeviceList,
    DeviceInfo { device_id: String },
    PeerList,
    PeerConnect { device_id: String },
    PeerDisconnect { device_id: String },
    PeerScan,
    MessageList,
    MessageSend { content: String },
    MessageDelete { message_id: String },
    NetworkStatus,
    NetworkAddresses,
    NetworkStart,
    NetworkStop,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonResponse {
    Status {
        device_id: String,
        device_name: String,
        authorized: bool,
        last_seen: String,
        connected_peers: usize,
        discovered_peers: usize,
        total_messages: usize,
        total_devices: usize,
    },
    DeviceList(Vec<DeviceInfo>),
    DeviceInfo(Option<DeviceInfo>),
    PeerList {
        connected: Vec<PeerInfo>,
        discovered: Vec<DiscoveredPeerInfo>,
    },
    MessageList(Vec<MessageInfo>),
    MessageSent { message_id: String },
    NetworkStatus {
        connected_peers: usize,
        discovered_peers: usize,
        active: bool,
    },
    NetworkAddresses(Vec<String>),
    Success,
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_name: String,
    pub is_authorized: bool,
    pub last_seen: String,
    pub is_online: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    pub device_id: String,
    pub device_name: String,
    pub connection_state: Option<String>,
    pub health: Option<String>,
    pub last_seen: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoveredPeerInfo {
    pub device_id: String,
    pub service_type: String,
    pub addresses: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageInfo {
    pub id: String,
    pub content: String,
    pub created_at: String,
}

pub struct DaemonServer {
    account: AccountHandle,
    control_port: u16,
}

impl DaemonServer {
    pub fn new(account: AccountHandle, control_port: u16) -> Self {
        Self {
            account,
            control_port,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.control_port));
        let listener = TcpListener::bind(addr).await?;
        info!("Daemon control server listening on: {}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    debug!("New client connected from: {}", addr);
                    // Handle client in the same task for now
                    if let Err(e) = Self::handle_client(stream, &mut self.account).await {
                        error!("Error handling client from {}: {}", addr, e);
                    }
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    async fn handle_client(mut stream: TcpStream, account: &mut AccountHandle) -> Result<()> {
        let mut buffer = vec![0; 4096];
        let n = stream.read(&mut buffer).await?;
        
        if n == 0 {
            return Ok(());
        }

        let request: DaemonRequest = serde_json::from_slice(&buffer[..n])?;
        debug!("Received request: {:?}", request);

        let response = Self::process_request(account, request).await;
        let response_json = serde_json::to_vec(&response)?;
        
        stream.write_all(&response_json).await?;
        stream.flush().await?;

        Ok(())
    }

    async fn process_request(
        account: &mut AccountHandle,
        request: DaemonRequest,
    ) -> DaemonResponse {
        match request {
            DaemonRequest::Status => {
                let device_info = account.device_info().await;
                let connected_count = account.connected_peers_count().await;
                let discovered_peers = account.discovered_peers().await;
                let messages = account.list_messages().await.unwrap_or_default();
                let devices = account.list_authorized_devices().await.unwrap_or_default();

                DaemonResponse::Status {
                    device_id: device_info.device_id,
                    device_name: device_info.device_name,
                    authorized: device_info.is_authorized,
                    last_seen: device_info.last_seen.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    connected_peers: connected_count as usize,
                    discovered_peers: discovered_peers.len(),
                    total_messages: messages.len(),
                    total_devices: devices.len(),
                }
            }
            DaemonRequest::DeviceList => {
                let devices = account.list_authorized_devices().await.unwrap_or_default();
                let device_infos: Vec<DeviceInfo> = devices
                    .into_iter()
                    .map(|d| DeviceInfo {
                        device_id: d.device_id,
                        device_name: d.device_name,
                        is_authorized: d.is_authorized,
                        last_seen: d.last_seen.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                        is_online: d.is_online,
                    })
                    .collect();
                DaemonResponse::DeviceList(device_infos)
            }
            DaemonRequest::DeviceInfo { device_id } => {
                let devices = account.list_authorized_devices().await.unwrap_or_default();
                let device = devices.into_iter().find(|d| d.device_id == device_id);
                let device_info = device.map(|d| DeviceInfo {
                    device_id: d.device_id,
                    device_name: d.device_name,
                    is_authorized: d.is_authorized,
                    last_seen: d.last_seen.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    is_online: d.is_online,
                });
                DaemonResponse::DeviceInfo(device_info)
            }
            DaemonRequest::PeerList => {
                let connected_peers = account.connected_peers().await;
                let discovered_peers = account.discovered_peers().await;

                let mut connected: Vec<PeerInfo> = Vec::new();
                for (device_id, device_info) in connected_peers {
                    let connection_state = account.peer_connection_state(&device_id).await;
                    let health = account.peer_health(&device_id).await;
                    connected.push(PeerInfo {
                        device_id: device_id.clone(),
                        device_name: device_info.device_name,
                        connection_state,
                        health,
                        last_seen: device_info.last_seen.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    });
                }

                let discovered: Vec<DiscoveredPeerInfo> = discovered_peers
                    .into_iter()
                    .map(|(device_id, discovery_info)| DiscoveredPeerInfo {
                        device_id,
                        service_type: format!("{:?}", discovery_info.service_type),
                        addresses: discovery_info.addresses,
                    })
                    .collect();

                DaemonResponse::PeerList { connected, discovered }
            }
            DaemonRequest::MessageList => {
                let messages = account.list_messages().await.unwrap_or_default();
                let message_infos: Vec<MessageInfo> = messages
                    .into_iter()
                    .map(|m| MessageInfo {
                        id: format!("{:?}", m.id),
                        content: m.content,
                        created_at: m.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    })
                    .collect();
                DaemonResponse::MessageList(message_infos)
            }
            DaemonRequest::MessageSend { content } => {
                match account.create_message(content, Vec::new()).await {
                    Ok(message_id) => DaemonResponse::MessageSent {
                        message_id: format!("{:?}", message_id),
                    },
                    Err(e) => DaemonResponse::Error(format!("Failed to send message: {}", e)),
                }
            }
            DaemonRequest::NetworkStatus => {
                let connected_count = account.connected_peers_count().await;
                let discovered_peers = account.discovered_peers().await;
                DaemonResponse::NetworkStatus {
                    connected_peers: connected_count as usize,
                    discovered_peers: discovered_peers.len(),
                    active: connected_count > 0 || !discovered_peers.is_empty(),
                }
            }
            DaemonRequest::PeerScan => {
                // TODO: Implement peer scanning
                DaemonResponse::Success
            }
            DaemonRequest::PeerConnect { device_id } => {
                // TODO: Implement peer connection
                DaemonResponse::Success
            }
            DaemonRequest::PeerDisconnect { device_id } => {
                // TODO: Implement peer disconnection
                DaemonResponse::Success
            }
            DaemonRequest::MessageDelete { message_id } => {
                // TODO: Implement message deletion
                DaemonResponse::Success
            }
            DaemonRequest::NetworkAddresses => {
                // TODO: Implement network addresses
                DaemonResponse::NetworkAddresses(vec!["Not implemented".to_string()])
            }
            DaemonRequest::NetworkStart => {
                // TODO: Implement network start
                DaemonResponse::Success
            }
            DaemonRequest::NetworkStop => {
                // TODO: Implement network stop
                DaemonResponse::Success
            }
        }
    }
}
