pub(crate) mod api;
pub(crate) mod handle;
mod relay_manager;
mod rpc_macros;

use crate::error::{SavedError, SavedResult};
use crate::network::api::{SavedNetworkApi, SavedNetworkRpc};
use crate::network::handle::SavedHandle;
use crate::network::relay_manager::RelayManager;
use crate::view::NetworkView;
use futures::StreamExt;
use libp2p::autonat::NatStatus;
use libp2p::core::ConnectedPoint;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::relay::client::Event::ReservationReqAccepted;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::swarm::{ConnectionId, NetworkBehaviour, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, autonat, dcutr, identify, kad, mdns, ping, relay};
use rand_core::OsRng;
use shutdown_signal::shutdown_signal;
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tokio::{select, time};

pub struct SavedNetwork {
    swarm: Swarm<SavedBehaviour>,
    view: NetworkView,
    /// AutoNAT
    nat_status: NatStatus,
    relay_manager: RelayManager,
    target_peers: HashSet<PeerId>,
    active_mode: bool,
    events_tx: broadcast::Sender<SavedNetworkEvent>,
    cmd_rx: mpsc::Receiver<SavedNetworkRpc>,
}

#[derive(NetworkBehaviour)]
struct SavedBehaviour {
    ping: ping::Behaviour,
    mdns: Toggle<mdns::tokio::Behaviour>,
    kad: Toggle<kad::Behaviour<kad::store::MemoryStore>>,
    identify: identify::Behaviour,
    autonat: autonat::Behaviour,
    autonat_v2: autonat::v2::client::Behaviour,
    dcutr: dcutr::Behaviour,
    relay: relay::client::Behaviour,
}

#[derive(Debug, Clone)]
pub enum SavedNetworkEvent {
    Connected {
        peer_id: PeerId,
        connection_id: ConnectionId,
    },
    Disconnected {
        peer_id: PeerId,
        connection_id: ConnectionId,
    },
    KadFoundPeer {
        target: PeerId,
        addresses: Vec<Multiaddr>,
    },
    ListeningOn {
        addr: Multiaddr,
    },
}

/// Set the [`KadMode`] in which we should operate.
///
/// With [`KadMode::Auto`], we are in [`Mode::Client`] and will swap into [`Mode::Server`] as
/// soon as we have a confirmed, external address via [`FromSwarm::ExternalAddrConfirmed`].
///
/// Setting a mode to [`KadMode::Auto`] or [`KadMode::Server`] disables this automatic
/// behaviour and unconditionally operates in the specified mode.
#[derive(Debug, Clone)]
pub enum KadMode {
    Auto,
    Client,
    Server,
}

fn should_skip_for_dial(addr: &Multiaddr) -> bool {
    // Skip IPv6 link-local without zone (needs %iface)
    let mut has_ip6_ll = false;
    let mut has_zone = false;
    for p in addr.iter() {
        match p {
            Protocol::Ip6(ip) if ip.is_unicast_link_local() => has_ip6_ll = true,
            Protocol::Ip6zone(_) => has_zone = true,
            _ => {}
        }
    }
    has_ip6_ll && !has_zone
}

impl SavedNetwork {
    pub async fn new(keypair: Keypair) -> SavedResult<SavedHandle> {
        let peer_id = keypair.public().to_peer_id();
        println!("Generated PeerId: {}", peer_id);

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair.clone())
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )?
            .with_quic()
            .with_dns()?
            .with_relay_client(libp2p::noise::Config::new, libp2p::yamux::Config::default)?
            .with_behaviour(|_keypair, relay_client| SavedBehaviour {
                ping: ping::Behaviour::new(
                    ping::Config::default().with_interval(Duration::from_secs(15)),
                ),
                mdns: Toggle::from(None),
                kad: Toggle::from(None),
                identify: identify::Behaviour::new(
                    identify::Config::new("/saved/0.1.0".into(), keypair.public())
                        .with_push_listen_addr_updates(true),
                ),
                autonat: autonat::Behaviour::new(
                    peer_id,
                    autonat::Config {
                        retry_interval: Duration::from_secs(10),
                        refresh_interval: Duration::from_secs(30),
                        boot_delay: Duration::from_secs(5),
                        throttle_server_period: Duration::ZERO,
                        only_global_ips: true,
                        ..Default::default()
                    },
                ),
                autonat_v2: autonat::v2::client::Behaviour::new(
                    OsRng,
                    autonat::v2::client::Config::default()
                        .with_probe_interval(Duration::from_secs(5)),
                ),
                dcutr: dcutr::Behaviour::new(peer_id),
                relay: relay_client,
            })?
            .build();

        // Listen on all interfaces (both IPv4 and IPv6) on ephemeral TCP ports.
        let listen_addr_v4: Multiaddr = "/ip4/0.0.0.0/tcp/0".parse()?;
        let listen_addr_v6: Multiaddr = "/ip6/::/tcp/0".parse()?;

        swarm.listen_on(listen_addr_v4)?;
        swarm.listen_on(listen_addr_v6)?;

        let (events_tx, _rx) = broadcast::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(64);

        let mut net = SavedNetwork {
            swarm,
            view: Default::default(),
            nat_status: NatStatus::Unknown,
            relay_manager: RelayManager::new(4),
            target_peers: Default::default(),
            active_mode: true,
            events_tx: events_tx.clone(),
            cmd_rx,
        };

        net.set_mdns_enabled(true).await?;
        net.set_kad_enabled(false, KadMode::Auto).await?;

        let net_run_handle = net.run().await;
        let handle = SavedHandle {
            id: peer_id,
            events_rx: events_tx.subscribe(),
            cmd_tx,
            task_handle: Some(net_run_handle),
        };

        Ok(handle)
    }

    async fn run(mut self) -> JoinHandle<()> {
        tokio::task::spawn(async move {
            self.event_loop().await;
        })
    }

    async fn event_loop(&mut self) {
        println!("Press Ctrl+C to stop.");

        let shutdown = shutdown_signal();
        tokio::pin!(shutdown);

        let mut ticker = time::interval(Duration::from_secs(5));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            select! {
                biased;
                reason = &mut shutdown =>  {
                    println!("Received shutdown signal: {:?}", reason);
                    break;
                }
                _ = ticker.tick() => {
                    println!("[heartbeat] is_active={}, peers={}", self.active_mode,
                        self.view.peers.values().map(|p| p.addresses
                                    .iter()
                                    .map(|a|
                                        (a.address.clone(), a.active_connections.len())
                                    )
                                    .filter(|(_, count)| *count > 0)
                                    .collect::<Vec<_>>())
                            .map(|x| format!("{:?}", x))
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    if let Err(e) = self.dispatch_rpc(cmd).await {
                        eprintln!("RPC error: {}", e);
                    }
                }
                event = self.swarm.select_next_some() => {
                    match self.on_swarm_event(event).await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Error handling swarm event: {}", e);
                        }
                    }
                }
            }
        }
    }

    async fn on_add_address(&mut self, peer_id: PeerId, addr: Multiaddr) {
        if should_skip_for_dial(&addr) {
            return;
        }
        if !self.view.add_address(peer_id, addr.clone()) {
            return;
        }
        println!("New address added to peer {peer_id}: {addr}");
        if let Some(kad) = self.swarm.behaviour_mut().kad.as_mut() {
            kad.add_address(&peer_id, addr.clone());
        }
        self.reconcile_peer_connections(peer_id);
    }

    async fn on_remove_address(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.view.remove_address(&peer_id, &addr);
        if let Some(kad) = self.swarm.behaviour_mut().kad.as_mut() {
            kad.remove_address(&peer_id, &addr);
        }
        self.reconcile_peer_connections(peer_id);
    }

    async fn on_peer_connection_closed(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
    ) -> SavedResult<()> {
        self.view.remove_connection(&peer_id, connection_id);
        self.events_tx.send(SavedNetworkEvent::Disconnected {
            peer_id,
            connection_id,
        })?;

        if self.target_peers.contains(&peer_id) {
            // Check if we still have a valid connection to this peer
            if !self.view.is_connected(&peer_id) {
                // No longer connected to this target peer, get active
                self.set_active_mode(true).await?;
            }
        }

        self.reconcile_peer_connections(peer_id);
        self.update_relay_state();

        Ok(())
    }

    async fn on_peer_connection_opened(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
    ) -> SavedResult<()> {
        self.view.add_connection(peer_id, connection_id, &endpoint);
        self.events_tx.send(SavedNetworkEvent::Connected {
            peer_id,
            connection_id,
        })?;

        if self.target_peers.contains(&peer_id) {
            let mut all_connected = true;
            for t in self.target_peers.iter() {
                if !self.view.is_connected(t) {
                    println!("Peer {t} is not connected, entering active mode");
                    self.set_active_mode(true).await?;
                    all_connected = false;
                    break;
                }
            }
            if all_connected {
                println!("All target peers are connected, leaving active mode");
                self.set_active_mode(false).await?;
            }
        }

        self.reconcile_peer_connections(peer_id);
        self.update_relay_state();

        Ok(())
    }

    fn is_prune_leader(&self, remote: &PeerId) -> bool {
        let me = self.swarm.local_peer_id();
        me.to_bytes() < remote.to_bytes()
    }

    /// Ensure we keep at most one connection per peer, on the currently-best address.
    fn reconcile_peer_connections(&mut self, peer_id: PeerId) {
        println!("Reconcile peer connections for {peer_id} called...");
        let Some(info) = self.view.peers.get(&peer_id) else {
            return;
        };

        if info.addresses.is_empty() {
            return;
        }

        // Best address is always at index 0 thanks to view.resort().
        let best_addr = info.addresses[0].address.clone();
        let best_is_up = self.view.is_connected_over(&peer_id, &best_addr);

        if best_is_up {
            if !self.is_prune_leader(&peer_id) {
                println!(
                    "Not my job to prune; keep status quo to avoid both sides closing at once."
                );
                return;
            }
            println!(
                "Peer {peer_id} is already connected with the best connection, killing suboptimal connections..."
            );
            let Some(info) = self.view.peers.get(&peer_id) else {
                return;
            };
            // Drop all non-best connections (keep only those on best_addr).
            let mut to_close: Vec<ConnectionId> = Vec::new();
            for meta in info.addresses.iter().skip(1) {
                to_close.extend(meta.active_connections.iter().copied());
            }
            for cid in to_close {
                // Close only the unwanted connections.
                let _ = self.swarm.close_connection(cid);
            }
        } else {
            println!(
                "Peer {peer_id} is not connected with the best connection, try dialing the best one..."
            );
            // If best isn't up but we have other connections, keep them for continuity
            // and also try to dial the best to migrate over when ready.
            // If nothing is connected at all, this will also initiate the first dial.
            if let Err(e) = self.swarm.dial(best_addr.clone()) {
                eprintln!("reconcile: dial({best_addr}) failed for {peer_id}: {e}");
                if let Some(p) = self.view.peers.get_mut(&peer_id) {
                    p.mark_failure(&best_addr);
                }
                self.reconcile_peer_connections(peer_id);
            }
        }
    }

    async fn set_active_mode(&mut self, active_mode: bool) -> SavedResult<()> {
        println!("Setting active mode to {}", active_mode);
        self.active_mode = active_mode;
        // TODO: This disrespects prior settings, I don't care about that for now, but might need to
        //  fix later
        if active_mode {
            self.set_kad_enabled(true, KadMode::Client).await?;
            self.set_mdns_enabled(true).await?;
            for t in self.target_peers.clone().into_iter() {
                self.kad_find_peer(t).await?;
            }
        } else {
            self.set_kad_enabled(false, KadMode::Auto).await?;
        }

        Ok(())
    }

    async fn on_nat_status_change(&mut self, new_status: NatStatus) -> SavedResult<()> {
        self.nat_status = new_status;
        Ok(())
    }

    /// Update relay usage state and prune if needed.
    /// Call this after connection state changes.
    fn update_relay_state(&mut self) {
        self.relay_manager.recompute_in_use(&self.view);
        self.prune_relays();
    }

    /// Prune excess relay reservations, preferring to keep low-latency relays.
    fn prune_relays(&mut self) {
        while self.relay_manager.should_prune() {
            if let Some(to_prune) = self.relay_manager.get_worst_unused(&self.view) {
                let rtt = self.relay_manager.get_relay_rtt(&self.view, &to_prune);
                self.relay_manager.remove(&to_prune);
                // Best-effort disconnect; reservation will lapse server-side
                let _ = self.swarm.disconnect_peer_id(to_prune);
                println!("🔌 Pruned relay reservation: {to_prune} (RTT: {rtt:?})");
            } else {
                // All reserved relays are carrying live traffic → stop pruning
                break;
            }
        }
    }

    async fn on_swarm_event(&mut self, event: SwarmEvent<SavedBehaviourEvent>) -> SavedResult<()> {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                // Also print a p2p multiaddr with our PeerId for easy copy-paste.
                let p2p_addr = address
                    .clone()
                    .with(Protocol::P2p(*self.swarm.local_peer_id()));
                println!("Listening on {p2p_addr}");
                self.events_tx
                    .send(SavedNetworkEvent::ListeningOn { addr: p2p_addr })?;
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::Ping(ev)) => {
                // Try to find the connection address from our view
                if let Some(peer_info) = self.view.peers.get_mut(&ev.peer) {
                    if let Some(addr) = peer_info.get_connection_address(&ev.connection) {
                        println!(
                            "🏓 Ping event: peer={} via {} -> {:?}",
                            ev.peer, addr, ev.result
                        );
                        if let Ok(rtt) = ev.result {
                            peer_info.rtt = rtt;
                        }
                    } else {
                        eprintln!(
                            "🏓 [!] Ping event: peer={} (connection {:?}, addr unknown) -> {:?}",
                            ev.peer, ev.connection, ev.result
                        );
                    }
                } else {
                    eprintln!(
                        "🏓 [!] Ping event: unknown peer {} (connection {:?}) -> {:?}",
                        ev.peer, ev.connection, ev.result
                    );
                }
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                // New peers discovered via mDNS - automatically connect to them
                for (peer_id, multiaddr) in list {
                    println!("mDNS discovered peer: {peer_id} at {multiaddr}");

                    self.on_add_address(peer_id, multiaddr.clone()).await;
                }
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                // Peers that are no longer available
                for (peer_id, multiaddr) in list {
                    println!("mDNS peer expired: {} at {}", peer_id, multiaddr);
                    self.on_remove_address(peer_id, multiaddr).await;
                }
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => {
                println!(
                    "✅ Connection established with peer: {} via {:?}",
                    peer_id, endpoint
                );

                // Check if this connection uses a relay
                let relay_used = match &endpoint {
                    ConnectedPoint::Dialer { address, .. } => {
                        RelayManager::extract_relay_peer(address)
                    }
                    ConnectedPoint::Listener { send_back_addr, .. } => {
                        RelayManager::extract_relay_peer(send_back_addr)
                    }
                };

                self.on_peer_connection_opened(peer_id, connection_id, endpoint)
                    .await?;

                // If this connection uses a relay, track it
                if let Some(relay_id) = relay_used {
                    println!("🔌 Connection via relay {relay_id}");
                    self.relay_manager.register(relay_id);
                    self.update_relay_state();
                }
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause,
                connection_id,
                ..
            } => {
                println!("❌ Connection closed with peer: {} - {:?}", peer_id, cause);
                self.on_peer_connection_closed(peer_id, connection_id)
                    .await?;
            }
            SwarmEvent::NewExternalAddrOfPeer { peer_id, address } => {
                // println!("🌐 Peer {peer_id} new external addr: {address}");
                self.on_add_address(peer_id, address.clone()).await;
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                println!("⚠️  Failed to connect to peer: {:?} - {:?}", peer_id, error);
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::Kad(
                kad::Event::OutboundQueryProgressed {
                    result: kad::QueryResult::GetClosestPeers(result),
                    ..
                },
            )) => match result {
                Ok(ok) => {
                    for p in &ok.peers {
                        println!("DHT Found Peer: {:?}", p);
                        self.events_tx.send(SavedNetworkEvent::KadFoundPeer {
                            target: p.peer_id,
                            addresses: p.addrs.clone(),
                        })?;
                        for addr in &p.addrs {
                            self.on_add_address(p.peer_id, addr.clone()).await;
                        }
                    }
                }
                Err(e) => {
                    println!("kad GetClosestPeers error: {e}");
                }
            },
            SwarmEvent::Behaviour(SavedBehaviourEvent::Identify(identify::Event::Received {
                peer_id,
                info,
                ..
            })) => {
                self.view.set_protocols(peer_id, info.protocols);
                for addr in info.listen_addrs {
                    // add to your view/Kad/address book and maybe dial
                    self.on_add_address(peer_id, addr).await;
                }

                // If peer supports relay HOP v2, check capacity before requesting reservation
                if self
                    .view
                    .peers
                    .get(&peer_id)
                    .map(|p| p.supports_relay_hop_v2)
                    .unwrap_or(false)
                {
                    // Check if we have capacity for this relay
                    if !self.relay_manager.can_reserve(&peer_id) {
                        println!(
                            "🔌 Skipping relay reservation for {peer_id}: at capacity ({} reserved, {} in use, max {})",
                            self.relay_manager.stats().reserved_count,
                            self.relay_manager.stats().in_use_count,
                            self.relay_manager.stats().max_relays
                        );
                        return Ok(());
                    }

                    if let Some(first_addr) = self
                        .view
                        .peers
                        .get(&peer_id)
                        .and_then(|p| p.addresses.first())
                    {
                        let relayed_listen = first_addr
                            .address
                            .clone()
                            .with(Protocol::P2p(peer_id))
                            .with(Protocol::P2pCircuit);

                        if let Err(e) = self.swarm.listen_on(relayed_listen.clone()) {
                            eprintln!(
                                "Failed to listen on relay {peer_id} @ {relayed_listen}: {:?}",
                                e
                            );
                        } else {
                            println!(
                                "🔌 Requesting relay reservation via {peer_id} @ {relayed_listen} ({:?})",
                                self.view.peers.get(&peer_id).map(|p| p.rtt)
                            );
                        }
                    }
                }
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::Relay(ReservationReqAccepted {
                relay_peer_id,
                renewal,
                limit,
            })) => {
                println!(
                    "🔌 Relay reservation accepted at {relay_peer_id}. Renewal: {renewal}. Limit: {limit:?}"
                );
                self.relay_manager.register(relay_peer_id);
                self.update_relay_state();
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::AutonatV2(autonat::v2::client::Event {
                server,
                tested_addr,
                bytes_sent,
                result,
            })) => match result {
                Ok(_) => {
                    println!(
                        "[AutoNATv2] Tested {tested_addr} with {server}. Sent {bytes_sent} bytes for verification. Everything Ok and verified."
                    );
                    self.on_nat_status_change(NatStatus::Public(tested_addr))
                        .await?;
                }
                Err(e) => {
                    println!(
                        "[AutoNATv2] Tested {tested_addr} with {server}. Sent {bytes_sent} bytes for verification. Failed with {e:?}."
                    );
                    self.on_nat_status_change(NatStatus::Private).await?;
                }
            },
            SwarmEvent::Behaviour(SavedBehaviourEvent::Autonat(
                autonat::Event::StatusChanged { old, new },
            )) => {
                println!("[AutoNAT] NAT status changed: {old:?}, {new:?}");
                self.on_nat_status_change(new).await?;
            }
            SwarmEvent::ExternalAddrConfirmed { address } => {
                println!("External address for this node confirmed: {address}");
            }
            _ => {
                println!("Unhandled event: {:?}", event);
            }
        }
        Ok(())
    }
}

impl SavedNetworkApi for SavedNetwork {
    async fn set_mdns_enabled(&mut self, enabled: bool) -> SavedResult<()> {
        let local = *self.swarm.local_peer_id();
        let mdns_toggle = &mut self.swarm.behaviour_mut().mdns;
        if enabled {
            if mdns_toggle.as_ref().is_none() {
                match mdns::tokio::Behaviour::new(mdns::Config::default(), local) {
                    Ok(b) => {
                        // Enable by replacing the Toggle with Some(behaviour)
                        *mdns_toggle = Toggle::from(Some(b));
                    }
                    Err(e) => {
                        eprintln!("failed to enable mDNS: {e}");
                        return Err(e.into());
                    }
                }
            }
        } else if mdns_toggle.as_ref().is_some() {
            // Disable by replacing with None
            *mdns_toggle = Toggle::from(None);
        }
        Ok(())
    }

    async fn set_kad_enabled(&mut self, enabled: bool, mode: KadMode) -> SavedResult<()> {
        println!("KAD enabled={enabled}, mode={mode:?}");
        let local = *self.swarm.local_peer_id();
        let kad_toggle = &mut self.swarm.behaviour_mut().kad;
        if enabled {
            if kad_toggle.as_ref().is_none() {
                let k = kad::Behaviour::with_config(
                    local,
                    kad::store::MemoryStore::new(local),
                    kad::Config::default(),
                );
                *kad_toggle = Toggle::from(Some(k));
            }
            let mode = match mode {
                KadMode::Client => Some(kad::Mode::Client),
                KadMode::Server => Some(kad::Mode::Server),
                KadMode::Auto => None,
            };
            if let Some(x) = kad_toggle.as_mut() {
                x.set_mode(mode)
            }
        } else if kad_toggle.as_ref().is_some() {
            // Disable by replacing with None
            *kad_toggle = Toggle::from(None);
        }
        Ok(())
    }

    async fn kad_find_peer(&mut self, target: PeerId) -> SavedResult<()> {
        if let Some(kad) = self.swarm.behaviour_mut().kad.as_mut() {
            kad.get_closest_peers(target);
            Ok(())
        } else {
            Err(SavedError::KadDisabled)
        }
    }

    async fn kad_bootstrap(&mut self) -> SavedResult<()> {
        if let Some(kad) = self.swarm.behaviour_mut().kad.as_mut() {
            kad.bootstrap()?;
            Ok(())
        } else {
            Err(SavedError::KadDisabled)
        }
    }

    async fn dial(&mut self, addr: Multiaddr) -> SavedResult<()> {
        // keep your guardrails
        let has_p2p = addr.iter().any(|p| matches!(p, Protocol::P2p(_)));
        if !has_p2p {
            return Err(SavedError::DialAddrMissingP2p(addr));
        }
        self.swarm.dial(addr)?;
        Ok(())
    }

    async fn add_target_peer(&mut self, target: PeerId) -> SavedResult<bool> {
        Ok(self.target_peers.insert(target))
    }

    async fn remove_target_peer(&mut self, target: PeerId) -> SavedResult<bool> {
        Ok(self.target_peers.remove(&target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keygen::keypair_from_seed;

    async fn make_net(seed: &str) -> SavedHandle {
        let keypair1 = keypair_from_seed(seed);

        SavedNetwork::new(keypair1.clone()).await.unwrap()
    }

    // Helper that waits until we see a Connected event for `target`.
    async fn wait_for_connected(rx: &mut broadcast::Receiver<SavedNetworkEvent>, target: PeerId) {
        loop {
            match rx.recv().await {
                Ok(SavedNetworkEvent::Connected { peer_id, .. }) if peer_id == target => break,
                Ok(_) => {} // other events; ignore
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("event channel closed unexpectedly: {e:?}"),
            }
        }
    }

    #[tokio::test]
    async fn test_two_nodes_mdns() {
        let mut network_handle1 = make_net("1").await;
        let mut network_handle2 = make_net("2").await;

        // Require both directions to observe a connection within 5 seconds.
        let both = async {
            tokio::join!(
                wait_for_connected(&mut network_handle1.events_rx, network_handle2.id),
                wait_for_connected(&mut network_handle2.events_rx, network_handle1.id),
            );
        };

        let res = tokio::time::timeout(Duration::from_secs(5), both).await;

        assert!(
            res.is_ok(),
            "nodes did not establish a connection within 5 seconds"
        );
    }

    #[tokio::test]
    async fn test_two_nodes_no_mdns() {
        let mut network_handle1 = make_net("1").await;
        network_handle1.set_mdns_enabled(false).await.unwrap();
        let mut network_handle2 = make_net("2").await;
        network_handle2.set_mdns_enabled(false).await.unwrap();

        // Require both directions to observe a connection within 5 seconds.
        let both = async {
            tokio::join!(
                wait_for_connected(&mut network_handle1.events_rx, network_handle1.id),
                wait_for_connected(&mut network_handle2.events_rx, network_handle2.id),
            );
        };

        let res = tokio::time::timeout(Duration::from_secs(5), both).await;

        assert!(
            res.is_err(),
            "nodes established a connection within 5 seconds, but should have been able to"
        );
    }

    #[tokio::test]
    async fn test_three_nodes_kad_peer_discovery_connects_via_middle() {
        // Spawn three nodes
        let mut a = make_net("A").await;
        let mut m = make_net("M").await;
        let mut c = make_net("C").await;

        a.add_target_peer(c.id).await.unwrap();
        c.add_target_peer(a.id).await.unwrap();

        // Disable mDNS for deterministic topology
        a.set_mdns_enabled(false).await.unwrap();
        m.set_mdns_enabled(false).await.unwrap();
        c.set_mdns_enabled(false).await.unwrap();

        // Enable Kad (server so they keep tables)
        a.set_kad_enabled(true, KadMode::Client).await.unwrap();
        m.set_kad_enabled(true, KadMode::Server).await.unwrap();
        c.set_kad_enabled(true, KadMode::Client).await.unwrap();

        // Wait for M to announce a p2p listening address
        let m_addr = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                match m.events_rx.recv().await {
                    Ok(SavedNetworkEvent::ListeningOn { addr }) => break addr,
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(e) => panic!("m events channel closed unexpectedly: {e:?}"),
                }
            }
        })
        .await
        .expect("middle node didn't start listening in time");

        // A and C connect to M using that addr
        a.dial(m_addr.clone()).await.unwrap();
        c.dial(m_addr.clone()).await.unwrap();

        // Wait for both A↔M and C↔M connections
        let am = async {
            tokio::join!(
                wait_for_connected(&mut a.events_rx, m.id),
                wait_for_connected(&mut m.events_rx, a.id),
            );
        };
        tokio::time::timeout(Duration::from_secs(1), am)
            .await
            .unwrap();
        let cm = async {
            tokio::join!(
                wait_for_connected(&mut c.events_rx, m.id),
                wait_for_connected(&mut m.events_rx, c.id),
            );
        };
        tokio::time::timeout(Duration::from_secs(1), cm)
            .await
            .unwrap();

        // Ask DHT on A to find C (routes via M); your Kad handler will
        // learn addrs and auto-dial via `on_add_address`.
        a.kad_find_peer(c.id).await.unwrap();

        // Finally, A should connect directly to C
        tokio::time::timeout(
            Duration::from_secs(1),
            wait_for_connected(&mut a.events_rx, c.id),
        )
        .await
        .expect("A did not connect to C via DHT discovery in time");
    }
}
