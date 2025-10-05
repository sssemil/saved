use crate::error::SavedResult;
use crate::signals::handle_shutdown_signals;
use crate::view::NetworkView;
use futures::StreamExt;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
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

    pub async fn dial(
        &self,
        addr: Multiaddr,
    ) -> Result<(), mpsc::error::SendError<SavedNetworkCommand>> {
        self.cmd_tx.send(SavedNetworkCommand::Dial(addr)).await
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
    mdns: mdns::tokio::Behaviour,
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
    Dial(Multiaddr),
}

impl SavedNetwork {
    pub async fn new(keypair: Keypair) -> SavedResult<SavedHandle> {
        let peer_id = keypair.public().to_peer_id();
        println!("Generated PeerId: {}", peer_id);

        let behaviour = SavedBehaviour {
            ping: ping::Behaviour::new(
                ping::Config::default().with_interval(Duration::from_secs(1)),
            ),
            mdns: mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)?,
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

        let net = SavedNetwork {
            swarm,
            view: Default::default(),
            events_tx: events_tx.clone(),
            cmd_rx,
        };
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
                    match cmd {
                        SavedNetworkCommand::Dial(addr) => {
                            if let Err(e) = self.swarm.dial(addr) {
                                eprintln!("dial error: {e}");
                            }
                        }
                    }
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

    #[tokio::test]
    async fn test_two_nodes_mdns() {
        let keypair1 = keypair_from_seed("1");
        let mut network_handle1 = SavedNetwork::new(keypair1.clone()).await.unwrap();

        let keypair2 = keypair_from_seed("2");
        let mut network_handle2 = SavedNetwork::new(keypair2.clone()).await.unwrap();

        // Subscribe BEFORE running so we don't miss early events.
        let rx1 = &mut network_handle1.events_rx;
        let rx2 = &mut network_handle2.events_rx;

        let id1 = keypair1.public().to_peer_id();
        let id2 = keypair2.public().to_peer_id();

        // Helper that waits until we see a Connected event for `target`.
        async fn wait_for_connected(
            rx: &mut broadcast::Receiver<SavedNetworkEvent>,
            target: PeerId,
        ) {
            loop {
                match rx.recv().await {
                    Ok(SavedNetworkEvent::Connected { peer_id, .. }) if peer_id == target => break,
                    Ok(_) => {} // other events; ignore
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(e) => panic!("event channel closed unexpectedly: {e:?}"),
                }
            }
        }

        // Require both directions to observe a connection within 5 seconds.
        let both = async {
            tokio::join!(wait_for_connected(rx1, id2), wait_for_connected(rx2, id1));
        };

        let res = tokio::time::timeout(Duration::from_secs(5), both).await;

        // Clean up the tasks no matter what, this should abort tasks.
        drop(network_handle1);
        drop(network_handle2);

        assert!(
            res.is_ok(),
            "nodes did not establish a connection within 5 seconds"
        );
    }
}
