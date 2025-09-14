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
        // TODO: Implement proper protobuf deserialization
        todo!("Implement OpsCodec::read_request")
    }

    fn read_response<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T) -> std::io::Result<Option<Self::Response>>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        // TODO: Implement proper protobuf deserialization
        todo!("Implement OpsCodec::read_response")
    }

    fn write_request<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T, req: Self::Request) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        // TODO: Implement proper protobuf serialization
        todo!("Implement OpsCodec::write_request")
    }

    fn write_response<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T, resp: Self::Response) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        // TODO: Implement proper protobuf serialization
        todo!("Implement OpsCodec::write_response")
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
        // TODO: Implement proper protobuf deserialization
        todo!("Implement ChunksCodec::read_request")
    }

    fn read_response<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T) -> std::io::Result<Option<Self::Response>>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        // TODO: Implement proper protobuf deserialization
        todo!("Implement ChunksCodec::read_response")
    }

    fn write_request<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T, req: Self::Request) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        // TODO: Implement proper protobuf serialization
        todo!("Implement ChunksCodec::write_request")
    }

    fn write_response<T>(&mut self, _: &request_response::ProtocolSupport, io: &mut T, resp: Self::Response) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        // TODO: Implement proper protobuf serialization
        todo!("Implement ChunksCodec::write_response")
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
            ops: operations.iter()
                .map(|op| {
                    // TODO: Encrypt operation for transmission
                    OpEnvelope {
                        header: None, // TODO: Fill header
                        ciphertext: Vec::new(), // TODO: Encrypt operation
                    }
                })
                .collect(),
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
            // TODO: Decrypt and verify operation
            // TODO: Add to event log
        }
        
        // Update heads
        self.event_sender.send(Event::HeadsUpdated)?;
        
        Ok(())
    }

    /// Handle chunks protocol message
    async fn handle_chunks_message(&mut self, message: request_response::Message<Vec<u8>, Vec<u8>>) -> Result<()> {
        // TODO: Implement chunks protocol handling
        todo!("Implement chunks protocol handling")
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DeviceKey;

    #[tokio::test]
    async fn test_network_manager_creation() {
        let device_key = DeviceKey::generate();
        let (event_sender, _event_receiver) = mpsc::channel();
        let event_log = EventLog::new();
        
        let network_manager = NetworkManager::new(device_key, event_sender, event_log).await;
        assert!(network_manager.is_ok());
    }
}
