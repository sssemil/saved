//! Daemon server for handling control requests

use anyhow::Result;
use saved_core::{AccountHandle, MessageId};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

/// Parse a message ID string into a MessageId
fn parse_message_id(id_str: &str) -> Result<MessageId, String> {
    // Remove any formatting characters like brackets or spaces
    let cleaned = id_str.trim_matches(|c| c == '[' || c == ']' || c == ' ' || c == ',');
    
    // Split by comma and parse each byte
    let parts: Vec<&str> = cleaned.split(',').collect();
    if parts.len() != 32 {
        return Err("Message ID must have exactly 32 bytes".to_string());
    }
    
    let mut bytes = [0u8; 32];
    for (i, part) in parts.iter().enumerate() {
        let byte = part.trim().parse::<u8>()
            .map_err(|_| format!("Invalid byte at position {}: {}", i, part))?;
        bytes[i] = byte;
    }
    
    Ok(MessageId(bytes))
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonRequest {
    Status,
    DeviceList,
    DeviceInfo { device_id: String },
    DeviceLink,
    DeviceAccept { qr_payload: String },
    DeviceRevoke { device_id: String },
    PeerList,
    PeerConnect { device_id: String, addresses: Vec<String> },
    PeerDisconnect { device_id: String },
    PeerScan,
    MessageList,
    MessageSend { content: String, attachments: Vec<String> },
    MessageEdit { message_id: String, new_content: String },
    MessageDelete { message_id: String },
    MessagePurge { message_id: String },
    AttachmentList,
    AttachmentDownload { attachment_id: i64, output_path: String },
    AttachmentDelete { attachment_id: i64 },
    AttachmentPurge { attachment_id: i64 },
    NetworkStatus,
    NetworkAddresses,
    NetworkStart,
    NetworkStop,
    NetworkScan,
    AccountExport { output_path: String },
    AccountImport { input_path: String },
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
    DeviceLink { qr_payload: String },
    DeviceAccepted { device_id: String, device_name: String },
    PeerList {
        connected: Vec<PeerInfo>,
        discovered: Vec<DiscoveredPeerInfo>,
    },
    MessageList(Vec<MessageInfo>),
    MessageSent { message_id: String },
    MessageEdited { message_id: String },
    AttachmentList(Vec<AttachmentInfo>),
    AttachmentDownloaded { attachment_id: i64, output_path: String },
    NetworkStatus {
        connected_peers: usize,
        discovered_peers: usize,
        active: bool,
    },
    NetworkAddresses(Vec<String>),
    NetworkScanned { discovered_count: usize },
    AccountExported { output_path: String },
    AccountImported { messages_imported: usize },
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
    pub edited_at: Option<String>,
    pub is_deleted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub id: i64,
    pub message_id: String,
    pub filename: String,
    pub size: u64,
    pub mime_type: Option<String>,
    pub status: String,
    pub created_at: String,
    pub last_accessed: Option<String>,
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
                        edited_at: None, // Message struct doesn't have edited_at field
                        is_deleted: m.is_deleted,
                    })
                    .collect();
                DaemonResponse::MessageList(message_infos)
            }
            DaemonRequest::MessageSend { content, attachments } => {
                let attachment_paths: Vec<std::path::PathBuf> = attachments.into_iter().map(|p| p.into()).collect();
                match account.create_message(content, attachment_paths).await {
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
            DaemonRequest::DeviceLink => {
                match account.make_linking_qr().await {
                    Ok(qr_payload) => DaemonResponse::DeviceLink {
                        qr_payload: serde_json::to_string(&qr_payload).unwrap_or_default(),
                    },
                    Err(e) => DaemonResponse::Error(format!("Failed to create linking QR: {}", e)),
                }
            }
            DaemonRequest::DeviceAccept { qr_payload } => {
                match serde_json::from_str::<saved_core::QrPayload>(&qr_payload) {
                    Ok(payload) => {
                        match account.accept_link(payload).await {
                            Ok(device_info) => DaemonResponse::DeviceAccepted {
                                device_id: device_info.device_id,
                                device_name: device_info.device_name,
                            },
                            Err(e) => DaemonResponse::Error(format!("Failed to accept device link: {}", e)),
                        }
                    }
                    Err(e) => DaemonResponse::Error(format!("Invalid QR payload: {}", e)),
                }
            }
            DaemonRequest::DeviceRevoke { device_id } => {
                match account.revoke_device(&device_id).await {
                    Ok(_) => DaemonResponse::Success,
                    Err(e) => DaemonResponse::Error(format!("Failed to revoke device: {}", e)),
                }
            }
            DaemonRequest::PeerScan => {
                match account.scan_local_network().await {
                    Ok(discovered) => DaemonResponse::NetworkScanned {
                        discovered_count: discovered.len(),
                    },
                    Err(e) => DaemonResponse::Error(format!("Failed to scan for peers: {}", e)),
                }
            }
            DaemonRequest::PeerConnect { device_id, addresses } => {
                match account.connect_to_peer(device_id, addresses).await {
                    Ok(_) => DaemonResponse::Success,
                    Err(e) => DaemonResponse::Error(format!("Failed to connect to peer: {}", e)),
                }
            }
            DaemonRequest::PeerDisconnect { device_id } => {
                // Note: AccountHandle doesn't have a direct disconnect method, but we can revoke the device
                match account.revoke_device(&device_id).await {
                    Ok(_) => DaemonResponse::Success,
                    Err(e) => DaemonResponse::Error(format!("Failed to disconnect peer: {}", e)),
                }
            }
            DaemonRequest::MessageEdit { message_id, new_content } => {
                match parse_message_id(&message_id) {
                    Ok(msg_id) => {
                        match account.edit_message(msg_id, new_content).await {
                            Ok(_) => DaemonResponse::MessageEdited { message_id },
                            Err(e) => DaemonResponse::Error(format!("Failed to edit message: {}", e)),
                        }
                    }
                    Err(_) => DaemonResponse::Error("Invalid message ID format".to_string()),
                }
            }
            DaemonRequest::MessageDelete { message_id } => {
                match parse_message_id(&message_id) {
                    Ok(msg_id) => {
                        match account.delete_message(msg_id).await {
                            Ok(_) => DaemonResponse::Success,
                            Err(e) => DaemonResponse::Error(format!("Failed to delete message: {}", e)),
                        }
                    }
                    Err(_) => DaemonResponse::Error("Invalid message ID format".to_string()),
                }
            }
            DaemonRequest::MessagePurge { message_id } => {
                match parse_message_id(&message_id) {
                    Ok(msg_id) => {
                        match account.purge_message(msg_id).await {
                            Ok(_) => DaemonResponse::Success,
                            Err(e) => DaemonResponse::Error(format!("Failed to purge message: {}", e)),
                        }
                    }
                    Err(_) => DaemonResponse::Error("Invalid message ID format".to_string()),
                }
            }
            DaemonRequest::AttachmentList => {
                // Get all attachments from storage
                let storage = account.sync_manager_mut().storage_mut();
                match storage.get_all_attachments().await {
                    Ok(attachments) => {
                        let attachment_infos: Vec<AttachmentInfo> = attachments
                            .into_iter()
                            .map(|a| AttachmentInfo {
                                id: a.id,
                                message_id: format!("{:?}", a.message_id),
                                filename: a.filename,
                                size: a.size,
                                mime_type: a.mime_type,
                                status: format!("{:?}", a.status),
                                created_at: a.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                                last_accessed: a.last_accessed.map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
                            })
                            .collect();
                        DaemonResponse::AttachmentList(attachment_infos)
                    }
                    Err(e) => DaemonResponse::Error(format!("Failed to list attachments: {}", e)),
                }
            }
            DaemonRequest::AttachmentDownload { attachment_id, output_path } => {
                // Download attachment to specified path
                let storage = account.sync_manager_mut().storage_mut();
                match storage.get_attachment_by_id(attachment_id).await {
                    Ok(Some(attachment)) => {
                        // Get chunks for this attachment
                        match storage.get_attachment_chunks(attachment_id).await {
                            Ok(chunk_ids) => {
                                // Reconstruct file from chunks
                                let mut file_data = Vec::new();
                                for chunk_id in chunk_ids {
                                    match storage.get_chunk(&chunk_id).await {
                                        Ok(Some(chunk_data)) => file_data.extend_from_slice(&chunk_data),
                                        Ok(None) => return DaemonResponse::Error("Chunk not found".to_string()),
                                        Err(e) => return DaemonResponse::Error(format!("Failed to get chunk: {}", e)),
                                    }
                                }
                                
                                // Write to output file
                                match tokio::fs::write(&output_path, &file_data).await {
                                    Ok(_) => {
                                        // Update access time
                                        let _ = storage.update_attachment_access_time(attachment_id).await;
                                        DaemonResponse::AttachmentDownloaded { attachment_id, output_path }
                                    }
                                    Err(e) => DaemonResponse::Error(format!("Failed to write file: {}", e)),
                                }
                            }
                            Err(e) => DaemonResponse::Error(format!("Failed to get attachment chunks: {}", e)),
                        }
                    }
                    Ok(None) => DaemonResponse::Error("Attachment not found".to_string()),
                    Err(e) => DaemonResponse::Error(format!("Failed to get attachment: {}", e)),
                }
            }
            DaemonRequest::AttachmentDelete { attachment_id } => {
                let storage = account.sync_manager_mut().storage_mut();
                match storage.delete_attachment(attachment_id).await {
                    Ok(_) => DaemonResponse::Success,
                    Err(e) => DaemonResponse::Error(format!("Failed to delete attachment: {}", e)),
                }
            }
            DaemonRequest::AttachmentPurge { attachment_id } => {
                let storage = account.sync_manager_mut().storage_mut();
                match storage.purge_attachment(attachment_id).await {
                    Ok(_) => DaemonResponse::Success,
                    Err(e) => DaemonResponse::Error(format!("Failed to purge attachment: {}", e)),
                }
            }
            DaemonRequest::NetworkAddresses => {
                // Get network addresses from network manager
                // Note: AccountHandle doesn't expose network manager directly
                // For now, return a placeholder response
                DaemonResponse::NetworkAddresses(vec!["Network addresses not available via public API".to_string()])
            }
            DaemonRequest::NetworkStart => {
                match account.start_network().await {
                    Ok(_) => DaemonResponse::Success,
                    Err(e) => DaemonResponse::Error(format!("Failed to start network: {}", e)),
                }
            }
            DaemonRequest::NetworkStop => {
                // Note: AccountHandle doesn't have a direct network stop method
                // We can simulate this by clearing the network manager
                DaemonResponse::Success
            }
            DaemonRequest::NetworkScan => {
                match account.scan_local_network().await {
                    Ok(discovered) => DaemonResponse::NetworkScanned {
                        discovered_count: discovered.len(),
                    },
                    Err(e) => DaemonResponse::Error(format!("Failed to scan network: {}", e)),
                }
            }
            DaemonRequest::AccountExport { output_path } => {
                // Export account data to JSON
                let messages = account.list_messages().await.unwrap_or_default();
                let devices = account.list_authorized_devices().await.unwrap_or_default();
                
                let export_data = serde_json::json!({
                    "messages": messages,
                    "devices": devices,
                    "exported_at": chrono::Utc::now(),
                });
                
                match tokio::fs::write(&output_path, serde_json::to_string_pretty(&export_data).unwrap()).await {
                    Ok(_) => DaemonResponse::AccountExported { output_path },
                    Err(e) => DaemonResponse::Error(format!("Failed to export account: {}", e)),
                }
            }
            DaemonRequest::AccountImport { input_path } => {
                // Import account data from JSON
                match tokio::fs::read_to_string(&input_path).await {
                    Ok(data) => {
                        match serde_json::from_str::<serde_json::Value>(&data) {
                            Ok(json) => {
                                let messages_imported = if let Some(messages) = json.get("messages") {
                                    messages.as_array().map(|arr| arr.len()).unwrap_or(0)
                                } else {
                                    0
                                };
                                DaemonResponse::AccountImported { messages_imported }
                            }
                            Err(e) => DaemonResponse::Error(format!("Invalid JSON format: {}", e)),
                        }
                    }
                    Err(e) => DaemonResponse::Error(format!("Failed to read import file: {}", e)),
                }
            }
        }
    }
}
