//! Client for communicating with the SAVED daemon

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
    InitializeChunkSync,
    StoreChunk { data: Vec<u8> },
    GetChunk { chunk_id: String },
    CheckChunkAvailability { chunk_ids: Vec<String> },
    FetchMissingChunks { chunk_ids: Vec<String> },
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
    ChunkSyncInitialized,
    ChunkStored { chunk_id: String },
    ChunkData { chunk_id: String, data: Option<Vec<u8>> },
    ChunkAvailability { availability: std::collections::HashMap<String, bool> },
    ChunksFetched { fetched_count: usize },
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

pub struct DaemonClient {
    control_port: u16,
}

impl DaemonClient {
    pub fn new(control_port: u16) -> Self {
        Self { control_port }
    }

    pub async fn send_request(&self, request: DaemonRequest) -> Result<DaemonResponse> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.control_port));
        let mut stream = TcpStream::connect(addr).await?;
        
        let request_json = serde_json::to_vec(&request)?;
        stream.write_all(&request_json).await?;
        stream.flush().await?;

        let mut buffer = vec![0; 4096];
        let n = stream.read(&mut buffer).await?;
        
        if n == 0 {
            return Err(anyhow::anyhow!("No response from daemon"));
        }

        let response: DaemonResponse = serde_json::from_slice(&buffer[..n])?;
        Ok(response)
    }

    pub async fn is_daemon_running(&self) -> bool {
        self.send_request(DaemonRequest::Status).await.is_ok()
    }
}
