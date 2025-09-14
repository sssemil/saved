//! Networking layer for SAVED using libp2p
//!
//! Implements:
//! - QUIC and TCP transports with Noise handshake
//! - mDNS discovery for LAN peers
//! - DCUtR for NAT traversal
//! - Relay v2 fallback
//! - Gossipsub for head announcements
//! - Request-response for operations and chunks

use crate::error::{Error, Result};
use crate::events::{Op, OpHash, EventLog};
use crate::protobuf::*;
use crate::types::{Event, DeviceInfo};
use libp2p::{
    identity, noise, quic, tcp, yamux, 
    mdns, identify, autonat, relay, dcutr,
    gossipsub, request_response,
    PeerId, Multiaddr, Transport, SwarmBuilder,
    swarm::{Swarm, dial_opts::DialOpts, NetworkBehaviour},
};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::Mutex;
use futures::StreamExt;
use futures::AsyncReadExt;
use futures::AsyncWriteExt;

/// Network manager for SAVED
pub struct NetworkManager {
    /// libp2p swarm
    swarm: Swarm<SavedBehaviour>,
    /// Event sender for notifying the application
    event_sender: mpsc::Sender<Event>,
    /// Connected peers
    connected_peers: HashMap<PeerId, DeviceInfo>,
    /// Event log for sync operations
    event_log: EventLog,
}

/// Combined libp2p behaviour for SAVED
#[derive(NetworkBehaviour)]
struct SavedBehaviour {
    /// mDNS for LAN discovery
    mdns: mdns::Behaviour,
    /// Identify protocol for peer information exchange
    identify: identify::Behaviour,
    /// AutoNAT for reachability assessment
    autonat: autonat::Behaviour,
    /// Relay v2 client for fallback connections
    relay: relay::client::Behaviour,
    /// DCUtR for NAT hole punching
    dcutr: dcutr::Behaviour,
    /// Gossipsub for head announcements
    gossipsub: gossipsub::Behaviour,
    /// Request-response for operations
    ops_protocol: request_response::Behaviour<OpsCodec>,
    /// Request-response for chunks
    chunks_protocol: request_response::Behaviour<ChunksCodec>,
}

/// Codec for operations protocol
#[derive(Clone)]
struct OpsCodec;

/// Codec for chunks protocol
#[derive(Clone)]
struct ChunksCodec;

impl request_response::Codec for OpsCodec {
    type Protocol = request_response::ProtocolSupport;
    type Request = Vec<u8>;
    type Response = Vec<u8>;

    fn read_request<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T) -> std::io::Result<Option<Self::Request>>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        // Read length-prefixed protobuf message
        let mut length_bytes = [0u8; 4];
        io.read_exact(&mut length_bytes).await?;
        let length = u32::from_be_bytes(length_bytes) as usize;
        
        let mut buffer = vec![0u8; length];
        io.read_exact(&mut buffer).await?;
        
        Ok(Some(buffer))
    }

    fn read_response<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T) -> std::io::Result<Option<Self::Response>>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        // Read length-prefixed protobuf message
        let mut length_bytes = [0u8; 4];
        io.read_exact(&mut length_bytes).await?;
        let length = u32::from_be_bytes(length_bytes) as usize;
        
        let mut buffer = vec![0u8; length];
        io.read_exact(&mut buffer).await?;
        
        Ok(Some(buffer))
    }

    fn write_request<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T, req: Self::Request) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        // Write length-prefixed protobuf message
        let length = req.len() as u32;
        io.write_all(&length.to_be_bytes()).await?;
        io.write_all(&req).await?;
        io.flush().await?;
        Ok(())
    }

    fn write_response<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T, resp: Self::Response) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        // Write length-prefixed protobuf message
        let length = resp.len() as u32;
        io.write_all(&length.to_be_bytes()).await?;
        io.write_all(&resp).await?;
        io.flush().await?;
        Ok(())
    }
}

impl request_response::Codec for ChunksCodec {
    type Protocol = request_response::ProtocolSupport;
    type Request = Vec<u8>;
    type Response = Vec<u8>;

    fn read_request<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T) -> std::io::Result<Option<Self::Request>>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        // Read length-prefixed protobuf message
        let mut length_bytes = [0u8; 4];
        io.read_exact(&mut length_bytes).await?;
        let length = u32::from_be_bytes(length_bytes) as usize;
        
        let mut buffer = vec![0u8; length];
        io.read_exact(&mut buffer).await?;
        
        Ok(Some(buffer))
    }

    fn read_response<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T) -> std::io::Result<Option<Self::Response>>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        // Read length-prefixed protobuf message
        let mut length_bytes = [0u8; 4];
        io.read_exact(&mut length_bytes).await?;
        let length = u32::from_be_bytes(length_bytes) as usize;
        
        let mut buffer = vec![0u8; length];
        io.read_exact(&mut buffer).await?;
        
        Ok(Some(buffer))
    }

    fn write_request<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T, req: Self::Request) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        // Write length-prefixed protobuf message
        let length = req.len() as u32;
        io.write_all(&length.to_be_bytes()).await?;
        io.write_all(&req).await?;
        io.flush().await?;
        Ok(())
    }

    fn write_response<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T, resp: Self::Response) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        // Write length-prefixed protobuf message
        let length = resp.len() as u32;
        io.write_all(&length.to_be_bytes()).await?;
        io.write_all(&resp).await?;
        io.flush().await?;
        Ok(())
    }
}

impl NetworkManager {
    /// Create a new network manager
    pub async fn new(
        device_key: crate::crypto::DeviceKey,
        event_sender: mpsc::Sender<Event>,
        event_log: EventLog,
    ) -> Result<Self> {
        // Create libp2p identity from device key
        let keypair = identity::Keypair::ed25519_from_bytes(device_key.private_key_bytes())
            .map_err(|e| Error::Network(format!("Failed to create keypair: {}", e)))?;
        
        let peer_id = keypair.public().to_peer_id();
        
        // Create transport
        let transport = Self::create_transport(keypair.clone())?;
        
        // Create behaviours
        let behaviour = Self::create_behaviour(keypair).await?;
        
        // Create swarm
        let swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, peer_id)
            .build();
        
        Ok(Self {
            swarm,
            event_sender,
            connected_peers: HashMap::new(),
            event_log,
        })
    }

    /// Create the libp2p transport
    fn create_transport(keypair: identity::Keypair) -> Result<Box<dyn Transport<Output = (PeerId, libp2p::StreamProtocol)>>> {
        // Noise configuration
        let noise_config = noise::Config::new(&keypair)
            .map_err(|e| Error::Network(format!("Failed to create noise config: {}", e)))?;
        
        // QUIC transport (preferred)
        let quic_transport = quic::tokio::Transport::new(quic::Config::new(&keypair));
        
        // TCP transport (fallback)
        let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise_config)
            .multiplex(yamux::Config::default())
            .boxed();
        
        // Combine transports
        let transport = quic_transport
            .or_transport(tcp_transport)
            .map(|(peer_id, muxer), _| (peer_id, muxer))
            .boxed();
        
        Ok(transport)
    }

    /// Create the combined behaviour
    async fn create_behaviour(keypair: identity::Keypair) -> Result<SavedBehaviour> {
        let peer_id = keypair.public().to_peer_id();
        
        // mDNS for LAN discovery
        let mdns = mdns::Behaviour::new(
            mdns::Config::default(),
            peer_id,
        ).map_err(|e| Error::Network(format!("Failed to create mDNS behaviour: {}", e)))?;
        
        // Identify protocol
        let identify = identify::Behaviour::new(identify::Config::new(
            "/savedmsgs/1.0.0".to_string(),
            keypair.public(),
        ));
        
        // AutoNAT for reachability assessment
        let autonat = autonat::Behaviour::new(
            peer_id,
            autonat::Config::default(),
        );
        
        // Relay v2 client
        let relay = relay::client::Behaviour::new(peer_id, Default::default());
        
        // DCUtR for NAT hole punching
        let dcutr = dcutr::Behaviour::new(peer_id);
        
        // Gossipsub for head announcements
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(1))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .message_id_fn(gossipsub::MessageId::from_slice)
            .build()
            .map_err(|e| Error::Network(format!("Failed to create gossipsub config: {}", e)))?;
        
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(keypair),
            gossipsub_config,
        ).map_err(|e| Error::Network(format!("Failed to create gossipsub behaviour: {}", e)))?;
        
        // Request-response protocols
        let ops_protocol = request_response::Behaviour::new(
            [(request_response::ProtocolSupport::Full, OpsCodec)],
            request_response::Config::default(),
        );
        
        let chunks_protocol = request_response::Behaviour::new(
            [(request_response::ProtocolSupport::Full, ChunksCodec)],
            request_response::Config::default(),
        );
        
        Ok(SavedBehaviour {
            mdns,
            identify,
            autonat,
            relay,
            dcutr,
            gossipsub,
            ops_protocol,
            chunks_protocol,
        })
    }

    /// Start listening on the given addresses
    pub async fn start_listening(&mut self, addresses: Vec<Multiaddr>) -> Result<()> {
        for addr in addresses {
            self.swarm.listen_on(addr)
                .map_err(|e| Error::Network(format!("Failed to listen on address: {}", e)))?;
        }
        Ok(())
    }

    /// Dial a peer
    pub async fn dial_peer(&mut self, peer_id: PeerId, addresses: Vec<Multiaddr>) -> Result<()> {
        let dial_opts = DialOpts::peer_id(peer_id)
            .addresses(addresses)
            .build();
        
        self.swarm.dial(dial_opts)
            .map_err(|e| Error::Network(format!("Failed to dial peer: {}", e)))?;
        
        Ok(())
    }

    /// Run the network event loop
    pub async fn run(&mut self) -> Result<()> {
        loop {
            match self.swarm.select_next_some().await {
                libp2p::swarm::SwarmEvent::Behaviour(event) => {
                    self.handle_behaviour_event(event).await?;
                }
                libp2p::swarm::SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    self.handle_connection_established(peer_id).await?;
                }
                libp2p::swarm::SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    self.handle_connection_closed(peer_id).await?;
                }
                libp2p::swarm::SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                    self.handle_outgoing_connection_error(peer_id, error).await?;
                }
                libp2p::swarm::SwarmEvent::IncomingConnectionError { .. } => {
                    // Log but don't fail
                }
                _ => {} // Handle other events as needed
            }
        }
    }

    /// Handle behaviour events
    async fn handle_behaviour_event(&mut self, event: <SavedBehaviour as NetworkBehaviour>::ToSwarm) -> Result<()> {
        match event {
            libp2p::swarm::ToSwarm::GenerateEvent(event) => {
                match event {
                    // Handle mDNS events
                    SavedBehaviourEvent::Mdns(mdns::Event::Discovered(list)) => {
                        for (peer_id, multiaddr) in list {
                            if !self.connected_peers.contains_key(&peer_id) {
                                self.dial_peer(peer_id, vec![multiaddr]).await?;
                            }
                        }
                    }
                    SavedBehaviourEvent::Mdns(mdns::Event::Expired(list)) => {
                        for (peer_id, _) in list {
                            self.connected_peers.remove(&peer_id);
                        }
                    }
                    
                    // Handle identify events
                    SavedBehaviourEvent::Identify(identify::Event::Received { peer_id, info }) => {
                        let device_info = DeviceInfo {
                            peer_id,
                            device_name: info.protocol_version,
                            last_seen: chrono::Utc::now(),
                            is_online: true,
                        };
                        self.connected_peers.insert(peer_id, device_info.clone());
                        self.event_sender.send(Event::Connected(device_info))?;
                    }
                    
                    // Handle gossipsub events
                    SavedBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. }) => {
                        self.handle_gossipsub_message(message).await?;
                    }
                    
                    // Handle request-response events
                    SavedBehaviourEvent::OpsProtocol(request_response::Event::Message { message, .. }) => {
                        self.handle_ops_message(message).await?;
                    }
                    SavedBehaviourEvent::ChunksProtocol(request_response::Event::Message { message, .. }) => {
                        self.handle_chunks_message(message).await?;
                    }
                    
                    _ => {} // Handle other events as needed
                }
            }
            libp2p::swarm::ToSwarm::Dial { opts } => {
                self.swarm.dial(opts)?;
            }
            libp2p::swarm::ToSwarm::ListenOn { opts } => {
                self.swarm.listen_on(opts)?;
            }
            _ => {} // Handle other ToSwarm events as needed
        }
        Ok(())
    }

    /// Handle connection established
    async fn handle_connection_established(&mut self, peer_id: PeerId) -> Result<()> {
        // Connection established, identify protocol will exchange peer info
        Ok(())
    }

    /// Handle connection closed
    async fn handle_connection_closed(&mut self, peer_id: PeerId) -> Result<()> {
        self.connected_peers.remove(&peer_id);
        self.event_sender.send(Event::Disconnected(peer_id))?;
        Ok(())
    }

    /// Handle outgoing connection error
    async fn handle_outgoing_connection_error(&mut self, peer_id: Option<PeerId>, error: libp2p::swarm::ConnectionError) -> Result<()> {
        // Log connection error but don't fail
        eprintln!("Connection error to {:?}: {:?}", peer_id, error);
        Ok(())
    }

    /// Handle gossipsub message
    async fn handle_gossipsub_message(&mut self, message: gossipsub::Message) -> Result<()> {
        // Parse announce heads message
        if let Ok(announce) = AnnounceHeads::decode(&message.data[..]) {
            self.handle_announce_heads(announce).await?;
        }
        Ok(())
    }

    /// Handle announce heads message
    async fn handle_announce_heads(&mut self, announce: AnnounceHeads) -> Result<()> {
        // Check if we need to sync operations
        let local_heads = self.event_log.get_heads();
        let remote_heads: HashSet<OpHash> = announce.heads.iter()
            .map(|h| {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&h[0..32]);
                hash
            })
            .collect();
        
        if local_heads != &remote_heads {
            // Need to sync - request operations
            self.request_operations(announce.since_heads).await?;
        }
        
        self.event_sender.send(Event::HeadsUpdated)?;
        Ok(())
    }

    /// Request operations from peers
    async fn request_operations(&mut self, since_heads: Vec<Vec<u8>>) -> Result<()> {
        let request = FetchOpsReq {
            since_heads,
            want_max: 256, // Default page size
        };
        
        let request_bytes = request.encode_to_vec();
        
        // Send request to all connected peers
        for peer_id in self.connected_peers.keys() {
            self.swarm.behaviour_mut().ops_protocol.send_request(peer_id, request_bytes.clone());
        }
        
        Ok(())
    }

    /// Handle operations protocol message
    async fn handle_ops_message(&mut self, message: request_response::Message<Vec<u8>, Vec<u8>>) -> Result<()> {
        match message {
            request_response::Message::Request { request, channel, peer } => {
                // Handle incoming request
                self.handle_ops_request(request, channel, peer).await?;
            }
            request_response::Message::Response { response, .. } => {
                // Handle incoming response
                self.handle_ops_response(response).await?;
            }
        }
        Ok(())
    }

    /// Handle operations request
    async fn handle_ops_request(
        &mut self,
        request: Vec<u8>,
        channel: request_response::ResponseChannel<Vec<u8>>,
        peer: PeerId,
    ) -> Result<()> {
        // Parse request
        let fetch_req = FetchOpsReq::decode(&request[..])
            .map_err(|e| Error::Network(format!("Failed to parse fetch ops request: {}", e)))?;
        
        // Get operations since the requested heads
        let since_heads: Vec<OpHash> = fetch_req.since_heads.iter()
            .map(|h| {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&h[0..32]);
                hash
            })
            .collect();
        
        let operations = self.event_log.get_operations_since(&since_heads);
        
        // Create response
        let response = FetchOpsResp {
            ops: {
                let mut encrypted_ops = Vec::new();
                for op in operations.iter() {
                    // Encrypt operation for transmission
                    let envelope = self.encrypt_operation_for_transmission(op).await?;
                    encrypted_ops.push(envelope);
                }
                encrypted_ops
            },
            new_heads: self.event_log.get_heads().iter()
                .map(|h| h.to_vec())
                .collect(),
        };
        
        let response_bytes = response.encode_to_vec();
        self.swarm.behaviour_mut().ops_protocol.send_response(channel, response_bytes);
        
        Ok(())
    }

    /// Handle operations response
    async fn handle_ops_response(&mut self, response: Vec<u8>) -> Result<()> {
        // Parse response
        let fetch_resp = FetchOpsResp::decode(&response[..])
            .map_err(|e| Error::Network(format!("Failed to parse fetch ops response: {}", e)))?;
        
        // Process received operations
        for envelope in fetch_resp.ops {
            // Decrypt and verify operation
            if let Some(op) = self.decrypt_and_verify_operation(&envelope).await? {
                self.event_log.add_operation(op)?;
            }
        }
        
        // Update heads
        self.event_sender.send(Event::HeadsUpdated)?;
        
        Ok(())
    }

    /// Handle chunks protocol message
    async fn handle_chunks_message(&mut self, message: request_response::Message<Vec<u8>, Vec<u8>>) -> Result<()> {
        match message {
            request_response::Message::Request { request, channel, peer } => {
                // Verify peer is authorized before processing request
                if !self.is_peer_authorized(&peer).await? {
                    return Err(Error::Network("Unauthorized peer".to_string()));
                }

                let fetch_req = FetchChunksReq::decode(&request[..])
                    .map_err(|e| Error::Network(format!("Failed to decode fetch chunks request: {}", e)))?;
                
                // Get chunks from storage (encrypted)
                let mut chunks = Vec::new();
                for cid in fetch_req.cids {
                    if cid.len() == 32 {
                        let mut chunk_id = [0u8; 32];
                        chunk_id.copy_from_slice(&cid);
                        // Get chunk from storage
                        if let Some(chunk_data) = self.storage.get_chunk(&chunk_id).await? {
                            chunks.push(fetch_chunks_resp::Chunk {
                                cid: cid.clone(),
                                data: chunk_data,
                            });
                        } else {
                            // Chunk not found, return empty chunk
                            chunks.push(fetch_chunks_resp::Chunk {
                                cid: cid.clone(),
                                data: Vec::new(),
                            });
                        }
                    }
                }
                
                let response = FetchChunksResp { chunks };
                let response_bytes = response.encode_to_vec();
                self.swarm.behaviour_mut().chunks_protocol.send_response(channel, response_bytes)
                    .map_err(|e| Error::Network(format!("Failed to send chunks response: {}", e)))?;
            }
            request_response::Message::Response { response, peer } => {
                // Verify peer is authorized before processing response
                if !self.is_peer_authorized(&peer).await? {
                    return Err(Error::Network("Unauthorized peer".to_string()));
                }

                let fetch_resp = FetchChunksResp::decode(&response[..])
                    .map_err(|e| Error::Network(format!("Failed to decode fetch chunks response: {}", e)))?;
                
                // Store received chunks
                for chunk in fetch_resp.chunks {
                    if chunk.cid.len() == 32 {
                        let mut chunk_id = [0u8; 32];
                        chunk_id.copy_from_slice(&chunk.cid);
                        // Store chunk in storage
                        self.storage.store_chunk(&chunk_id, &chunk.data).await?;
                    }
                }
            }
        }
        
        Ok(())
    }

    /// Announce current heads to all connected peers
    pub async fn announce_heads(&mut self) -> Result<()> {
        let heads = self.event_log.get_heads();
        let announce = AnnounceHeads {
            feed_id: "default".to_string(),
            lamport: self.event_log.current_lamport(),
            heads: heads.iter().map(|h| h.to_vec()).collect(),
        };
        
        let message_bytes = announce.encode_to_vec();
        
        // Publish to gossipsub
        let topic = gossipsub::IdentTopic::new("/savedmsgs/default/heads");
        self.swarm.behaviour_mut().gossipsub.publish(topic, message_bytes)
            .map_err(|e| Error::Network(format!("Failed to publish heads: {}", e)))?;
        
        Ok(())
    }

    /// Get connected peers
    pub fn connected_peers(&self) -> &HashMap<PeerId, DeviceInfo> {
        &self.connected_peers
    }

    /// Check if a peer is authorized for this account
    async fn is_peer_authorized(&self, peer_id: &PeerId) -> Result<bool> {
        // Convert peer ID to device ID format
        let peer_bytes = peer_id.to_bytes();
        let device_id = format!("peer_{:02x}{:02x}{:02x}{:02x}", 
            peer_bytes[0], peer_bytes[1], peer_bytes[2], peer_bytes[3]);
        
        // Check if this peer is in the authorized devices list
        self.storage.is_device_authorized(&device_id).await
    }

    /// Encrypt operation for transmission to authorized peer
    async fn encrypt_operation_for_transmission(&self, op: &Op) -> Result<OpEnvelope> {
        // Serialize the operation
        let op_bytes = bincode::serialize(op)
            .map_err(|e| Error::Crypto(format!("Failed to serialize operation: {}", e)))?;
        
        // Create a proper header with signature
        let header = self.create_operation_header(&op_bytes).await?;
        
        // Encrypt the operation with a derived key
        let encrypted_data = self.encrypt_with_derived_key(&op_bytes).await?;
        
        Ok(OpEnvelope {
            header: Some(header),
            ciphertext: encrypted_data,
        })
    }

    /// Create operation header with signature
    async fn create_operation_header(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Create a simple header with timestamp and data hash
        let timestamp = chrono::Utc::now().timestamp();
        let data_hash = crate::crypto::blake3_hash(data);
        
        // Create header structure
        let header = crate::protobuf::OpHeader {
            timestamp,
            data_hash: data_hash.to_vec(),
            signature: Vec::new(), // TODO: Add actual signature with device key
        };
        
        Ok(header.encode_to_vec())
    }

    /// Encrypt data with derived key
    async fn encrypt_with_derived_key(&self, data: &[u8]) -> Result<Vec<u8>> {
        // For now, use a simple XOR with a derived key
        // In a real implementation, this would use proper encryption
        let key = self.derive_transmission_key().await?;
        let mut encrypted = data.to_vec();
        
        for (i, byte) in encrypted.iter_mut().enumerate() {
            *byte ^= key[i % key.len()];
        }
        
        Ok(encrypted)
    }

    /// Decrypt data with derived key
    async fn decrypt_with_derived_key(&self, encrypted_data: &[u8]) -> Result<Vec<u8>> {
        // XOR is symmetric, so decryption is the same as encryption
        self.encrypt_with_derived_key(encrypted_data).await
    }

    /// Derive transmission key for encryption
    async fn derive_transmission_key(&self) -> Result<Vec<u8>> {
        // For now, use a simple key derivation
        // In a real implementation, this would derive from peer keys
        let mut key = vec![0u8; 32];
        for i in 0..32 {
            key[i] = (i as u8).wrapping_add(42);
        }
        Ok(key)
    }

    /// Decrypt and verify operation from peer
    async fn decrypt_and_verify_operation(&self, envelope: &OpEnvelope) -> Result<Option<Op>> {
        if envelope.ciphertext.is_empty() {
            return Ok(None);
        }
        
        // Verify the header if present
        if let Some(header_bytes) = &envelope.header {
            self.verify_operation_header(header_bytes).await?;
        }
        
        // Decrypt the operation
        let decrypted_data = self.decrypt_with_derived_key(&envelope.ciphertext).await?;
        
        // Deserialize the operation
        let op: Op = bincode::deserialize(&decrypted_data)
            .map_err(|e| Error::Crypto(format!("Failed to deserialize operation: {}", e)))?;
        
        Ok(Some(op))
    }

    /// Verify operation header
    async fn verify_operation_header(&self, header_bytes: &[u8]) -> Result<()> {
        // Parse the header
        let header = crate::protobuf::OpHeader::decode(header_bytes)
            .map_err(|e| Error::Crypto(format!("Failed to decode operation header: {}", e)))?;
        
        // Check timestamp (prevent replay attacks)
        let now = chrono::Utc::now().timestamp();
        let time_diff = (now - header.timestamp).abs();
        if time_diff > 300 { // 5 minutes tolerance
            return Err(Error::Crypto("Operation timestamp too old".to_string()));
        }
        
        // TODO: Verify signature with peer's public key
        // For now, just check that signature field exists
        if header.signature.is_empty() {
            return Err(Error::Crypto("Missing operation signature".to_string()));
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DeviceKey;
    use futures::io::Cursor;
    use std::io::Result as IoResult;

    #[tokio::test]
    async fn test_network_manager_creation() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::channel();
        let event_log = EventLog::new();
        
        let network_manager = NetworkManager::new(device_key, event_sender, event_log).await;
        assert!(network_manager.is_ok());
    }

    #[tokio::test]
    async fn test_ops_codec_read_request() {
        let mut codec = OpsCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        // Create test data
        let test_data = vec![1, 2, 3, 4, 5];
        let length = test_data.len() as u32;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&length.to_be_bytes());
        buffer.extend_from_slice(&test_data);
        
        let mut cursor = Cursor::new(buffer);
        let result = codec.read_request(&protocol, &mut cursor).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(test_data));
    }

    #[tokio::test]
    async fn test_ops_codec_read_response() {
        let mut codec = OpsCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        // Create test data
        let test_data = vec![5, 4, 3, 2, 1];
        let length = test_data.len() as u32;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&length.to_be_bytes());
        buffer.extend_from_slice(&test_data);
        
        let mut cursor = Cursor::new(buffer);
        let result = codec.read_response(&protocol, &mut cursor).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(test_data));
    }

    #[tokio::test]
    async fn test_ops_codec_write_request() {
        let mut codec = OpsCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        let test_data = vec![1, 2, 3, 4, 5];
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        let result = codec.write_request(&protocol, &mut cursor, test_data.clone()).await;
        assert!(result.is_ok());
        
        // Verify the written data
        let expected_length = test_data.len() as u32;
        let expected_bytes = {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&expected_length.to_be_bytes());
            bytes.extend_from_slice(&test_data);
            bytes
        };
        
        assert_eq!(buffer, expected_bytes);
    }

    #[tokio::test]
    async fn test_ops_codec_write_response() {
        let mut codec = OpsCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        let test_data = vec![5, 4, 3, 2, 1];
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        let result = codec.write_response(&protocol, &mut cursor, test_data.clone()).await;
        assert!(result.is_ok());
        
        // Verify the written data
        let expected_length = test_data.len() as u32;
        let expected_bytes = {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&expected_length.to_be_bytes());
            bytes.extend_from_slice(&test_data);
            bytes
        };
        
        assert_eq!(buffer, expected_bytes);
    }

    #[tokio::test]
    async fn test_chunks_codec_read_request() {
        let mut codec = ChunksCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        // Create test data
        let test_data = vec![10, 20, 30, 40, 50];
        let length = test_data.len() as u32;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&length.to_be_bytes());
        buffer.extend_from_slice(&test_data);
        
        let mut cursor = Cursor::new(buffer);
        let result = codec.read_request(&protocol, &mut cursor).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(test_data));
    }

    #[tokio::test]
    async fn test_chunks_codec_read_response() {
        let mut codec = ChunksCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        // Create test data
        let test_data = vec![50, 40, 30, 20, 10];
        let length = test_data.len() as u32;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&length.to_be_bytes());
        buffer.extend_from_slice(&test_data);
        
        let mut cursor = Cursor::new(buffer);
        let result = codec.read_response(&protocol, &mut cursor).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(test_data));
    }

    #[tokio::test]
    async fn test_chunks_codec_write_request() {
        let mut codec = ChunksCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        let test_data = vec![10, 20, 30, 40, 50];
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        let result = codec.write_request(&protocol, &mut cursor, test_data.clone()).await;
        assert!(result.is_ok());
        
        // Verify the written data
        let expected_length = test_data.len() as u32;
        let expected_bytes = {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&expected_length.to_be_bytes());
            bytes.extend_from_slice(&test_data);
            bytes
        };
        
        assert_eq!(buffer, expected_bytes);
    }

    #[tokio::test]
    async fn test_chunks_codec_write_response() {
        let mut codec = ChunksCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        let test_data = vec![50, 40, 30, 20, 10];
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        let result = codec.write_response(&protocol, &mut cursor, test_data.clone()).await;
        assert!(result.is_ok());
        
        // Verify the written data
        let expected_length = test_data.len() as u32;
        let expected_bytes = {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&expected_length.to_be_bytes());
            bytes.extend_from_slice(&test_data);
            bytes
        };
        
        assert_eq!(buffer, expected_bytes);
    }

    #[tokio::test]
    async fn test_codec_empty_data() {
        let mut ops_codec = OpsCodec::default();
        let mut chunks_codec = ChunksCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        // Test empty data
        let empty_data = vec![];
        let length = empty_data.len() as u32;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&length.to_be_bytes());
        buffer.extend_from_slice(&empty_data);
        
        let mut cursor = Cursor::new(buffer.clone());
        let result = ops_codec.read_request(&protocol, &mut cursor).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(empty_data.clone()));
        
        let mut cursor = Cursor::new(buffer);
        let result = chunks_codec.read_request(&protocol, &mut cursor).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(empty_data));
    }

    #[tokio::test]
    async fn test_codec_large_data() {
        let mut ops_codec = OpsCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        // Test with larger data
        let large_data = vec![42u8; 10000];
        let length = large_data.len() as u32;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&length.to_be_bytes());
        buffer.extend_from_slice(&large_data);
        
        let mut cursor = Cursor::new(buffer);
        let result = ops_codec.read_request(&protocol, &mut cursor).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(large_data));
    }

    #[tokio::test]
    async fn test_codec_roundtrip() {
        let mut ops_codec = OpsCodec::default();
        let protocol = request_response::ProtocolSupport::default();
        
        let original_data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        
        // Write the data
        let mut write_buffer = Vec::new();
        let mut write_cursor = Cursor::new(&mut write_buffer);
        let write_result = ops_codec.write_request(&protocol, &mut write_cursor, original_data.clone()).await;
        assert!(write_result.is_ok());
        
        // Read the data back
        let mut read_cursor = Cursor::new(write_buffer);
        let read_result = ops_codec.read_request(&protocol, &mut read_cursor).await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), Some(original_data));
    }
}
