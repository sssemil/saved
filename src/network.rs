use crate::error::SavedResult;
use crate::signals::handle_shutdown_signals;
use crate::view::NetworkView;
use futures::StreamExt;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::swarm::{ConnectionId, NetworkBehaviour, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, mdns, ping};
use std::time::Duration;
use tokio::select;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

pub struct SavedNetwork {
    swarm: Swarm<SavedBehaviour>,
    view: NetworkView,
    events_tx: broadcast::Sender<SavedNetworkEvent>,
    cmd_rx: mpsc::Receiver<SavedNetworkCommand>,
}

pub struct SavedHandle {
    pub events_rx: broadcast::Receiver<SavedNetworkEvent>,
    cmd_tx: mpsc::Sender<SavedNetworkCommand>,
    task_handle: Option<JoinHandle<()>>,
}

impl SavedHandle {
    pub async fn join(mut self) {
        if let Some(handle) = self.task_handle.take() {
            // Await consumes the JoinHandle; after take() we own it.
            let _ = handle.await;
        }
    }

    pub async fn set_mdns_enabled(
        &self,
        enabled: bool,
    ) -> Result<(), mpsc::error::SendError<SavedNetworkCommand>> {
        self.cmd_tx
            .send(SavedNetworkCommand::SetMdnsEnabled(enabled))
            .await
    }
}

impl Drop for SavedHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.take() {
            println!("Dropping save handle...");
            handle.abort();
            println!("Dropped save handle!");
        }
    }
}

#[derive(NetworkBehaviour)]
struct SavedBehaviour {
    ping: ping::Behaviour,
    mdns: Toggle<mdns::tokio::Behaviour>,
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
}

#[derive(Debug, Clone)]
pub enum SavedNetworkCommand {
    SetMdnsEnabled(bool),
}

impl SavedNetwork {
    pub async fn new(keypair: Keypair) -> SavedResult<SavedHandle> {
        let peer_id = keypair.public().to_peer_id();
        println!("Generated PeerId: {}", peer_id);

        let behaviour = SavedBehaviour {
            ping: ping::Behaviour::new(
                ping::Config::default().with_interval(Duration::from_secs(1)),
            ),
            mdns: Toggle::from(None),
        };

        let mut swarm: Swarm<SavedBehaviour> =
            libp2p::SwarmBuilder::with_existing_identity(keypair)
                .with_tokio()
                .with_tcp(
                    libp2p::tcp::Config::default(),
                    libp2p::noise::Config::new,
                    libp2p::yamux::Config::default,
                )?
                .with_behaviour(|_| behaviour)?
                .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(3)))
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
            events_tx: events_tx.clone(),
            cmd_rx,
        };

        net.set_mdns_enabled(true).await;

        let net_run_handle = net.run().await;
        let handle = SavedHandle {
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

        loop {
            select! {
                biased;
                Some(cmd) = self.cmd_rx.recv() => {
                    self.on_handle_cmd(cmd).await;
                }
                event = self.swarm.select_next_some() => {
                    self.on_swarm_event(event).await;
                }
                _ = handle_shutdown_signals() => {
                    println!("Received shutdown signal.");
                }
            }
        }
    }

    async fn set_mdns_enabled(&mut self, enabled: bool) {
        let local = *self.swarm.local_peer_id();
        let mdns_toggle = &mut self.swarm.behaviour_mut().mdns;
        if enabled {
            if mdns_toggle.as_ref().is_none() {
                match mdns::tokio::Behaviour::new(mdns::Config::default(), local) {
                    Ok(b) => {
                        // Enable by replacing the Toggle with Some(behaviour)
                        *mdns_toggle = Toggle::from(Some(b));
                    }
                    Err(e) => eprintln!("failed to enable mDNS: {e}"),
                }
            }
        } else if mdns_toggle.as_ref().is_some() {
            // Disable by replacing with None
            *mdns_toggle = Toggle::from(None);
        }
    }

    async fn on_handle_cmd(&mut self, cmd: SavedNetworkCommand) {
        match cmd {
            SavedNetworkCommand::SetMdnsEnabled(enabled) => {
                self.set_mdns_enabled(enabled).await;
            }
        }
    }

    async fn on_swarm_event(&mut self, event: SwarmEvent<SavedBehaviourEvent>) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                // Also print a p2p multiaddr with our PeerId for easy copy-paste.
                let p2p_addr = address
                    .clone()
                    .with(Protocol::P2p(*self.swarm.local_peer_id()));
                println!("Listening on {p2p_addr}");
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::Ping(ev)) => {
                // Basic visibility that the node is alive and pinging.
                println!("Ping event: peer={} result={:?}", ev.peer, ev.result);
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                // New peers discovered via mDNS - automatically connect to them
                for (peer_id, multiaddr) in list {
                    println!("mDNS discovered peer: {peer_id} at {multiaddr}");

                    self.view.add_address(peer_id, multiaddr.clone());

                    if self.view.is_connected_over(&peer_id, &multiaddr) {
                        println!(
                            "mDNS discovered an already connected peer address, skipping: {peer_id} -> {multiaddr}"
                        );
                    } else {
                        // Attempt to dial the discovered peer
                        match self.swarm.dial(multiaddr) {
                            Ok(()) => {
                                println!("Attempting to connect to discovered peer: {}", peer_id);
                            }
                            Err(e) => {
                                println!("Failed to dial discovered peer {}: {}", peer_id, e);
                            }
                        }
                    }
                }
            }
            SwarmEvent::Behaviour(SavedBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                // Peers that are no longer available
                for (peer_id, multiaddr) in list {
                    println!("mDNS peer expired: {} at {}", peer_id, multiaddr);
                    self.view.remove_address(&peer_id, &multiaddr);
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
                self.view.add_connection(peer_id, connection_id, &endpoint);
                let _ = self.events_tx.send(SavedNetworkEvent::Connected {
                    peer_id,
                    connection_id,
                });
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause,
                connection_id,
                ..
            } => {
                println!("❌ Connection closed with peer: {} - {:?}", peer_id, cause);
                self.view.remove_connection(&peer_id, connection_id);
                let _ = self.events_tx.send(SavedNetworkEvent::Disconnected {
                    peer_id,
                    connection_id,
                });
            }
            SwarmEvent::NewExternalAddrOfPeer { peer_id, address } => {
                println!("🌐 Peer {peer_id} new external addr: {address}");
                self.view.add_address(peer_id, address);
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                println!("⚠️  Failed to connect to peer: {:?} - {:?}", peer_id, error);
            }
            _ => {
                println!("Unhandled event: {:?}", event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keygen::keypair_from_seed;

    async fn make_net(seed: &str) -> (Keypair, SavedHandle, PeerId) {
        let keypair1 = keypair_from_seed(seed);
        let network_handle1 = SavedNetwork::new(keypair1.clone()).await.unwrap();
        let id1 = keypair1.public().to_peer_id();

        (keypair1, network_handle1, id1)
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
        let (_keypair1, mut network_handle1, id1) = make_net("1").await;
        let (_keypair2, mut network_handle2, id2) = make_net("2").await;

        // Require both directions to observe a connection within 5 seconds.
        let both = async {
            tokio::join!(
                wait_for_connected(&mut network_handle1.events_rx, id2),
                wait_for_connected(&mut network_handle2.events_rx, id1)
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
        let (_keypair1, mut network_handle1, id1) = make_net("1").await;
        network_handle1.set_mdns_enabled(false).await.unwrap();
        let (_keypair2, mut network_handle2, id2) = make_net("2").await;
        network_handle2.set_mdns_enabled(false).await.unwrap();

        // Require both directions to observe a connection within 5 seconds.
        let both = async {
            tokio::join!(
                wait_for_connected(&mut network_handle1.events_rx, id2),
                wait_for_connected(&mut network_handle2.events_rx, id1)
            );
        };

        let res = tokio::time::timeout(Duration::from_secs(5), both).await;

        assert!(
            res.is_err(),
            "nodes established a connection within 5 seconds, but should have been able to"
        );
    }
}
