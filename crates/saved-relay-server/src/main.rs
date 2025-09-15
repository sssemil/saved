use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use libp2p::{
    core::{multiaddr::Protocol, Multiaddr},
    identify, identity, noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use log::{info, warn};
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Parser)]
#[command(name = "saved-relay-server")]
#[command(about = "SAVED Public Relay Server - Accepts all traffic for hole punching")]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Fixed value to generate deterministic peer id
    #[arg(long, default_value = "1")]
    secret_key_seed: u8,

    /// Use IPv6 instead of IPv4
    #[arg(long)]
    use_ipv6: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    if cli.verbose {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Debug)
            .init();
    } else {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Info)
            .init();
    }

    info!("Starting SAVED Public Relay Server...");
    info!("Host: {}", cli.host);
    info!("Port: {}", cli.port);

    // Create a deterministic keypair for the relay server
    let local_key = generate_ed25519(cli.secret_key_seed);
    let peer_id = local_key.public().to_peer_id();
    info!("Relay Server Peer ID: {}", peer_id);

    // Build the libp2p swarm with relay behavior
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| Behaviour {
            relay: relay::Behaviour::new(key.public().to_peer_id(), Default::default()),
            ping: ping::Behaviour::new(ping::Config::new()),
            identify: identify::Behaviour::new(identify::Config::new(
                "saved-relay/1.0.0".to_string(),
                key.public(),
            )),
        })?
        .build();

    // Listen on the specified address
    let listen_addr = Multiaddr::empty()
        .with(match cli.use_ipv6 {
            true => Protocol::from(Ipv6Addr::UNSPECIFIED),
            false => Protocol::from(Ipv4Addr::UNSPECIFIED),
        })
        .with(Protocol::Tcp(cli.port));

    swarm.listen_on(listen_addr.clone())?;
    info!("Relay server listening on: {}", listen_addr);

    // Print the relay address for clients to use
    println!("🚀 SAVED Public Relay Server Started!");
    println!("📡 Relay Address: {}/p2p/{}", listen_addr, peer_id);
    println!("🔓 Accepting all traffic - no restrictions");
    println!("⚡ Ready for hole punching connections!");
    println!();

    // Main event loop
    loop {
        match swarm.next().await.expect("Infinite Stream.") {
            SwarmEvent::Behaviour(event) => {
                if let BehaviourEvent::Identify(identify::Event::Received {
                    info: identify::Info { observed_addr, .. },
                    ..
                }) = &event
                {
                    swarm.add_external_address(observed_addr.clone());
                }

                // Log relay events
                match &event {
                    BehaviourEvent::Relay(relay::Event::ReservationReqAccepted {
                        src_peer_id,
                        ..
                    }) => {
                        info!("✅ Reservation request accepted from peer: {}", src_peer_id);
                    }
                    BehaviourEvent::Relay(relay::Event::ReservationReqDenied {
                        src_peer_id,
                        ..
                    }) => {
                        warn!("❌ Reservation request denied from peer: {}", src_peer_id);
                    }
                    BehaviourEvent::Relay(relay::Event::CircuitReqAccepted {
                        src_peer_id,
                        dst_peer_id,
                        ..
                    }) => {
                        info!("🔗 Circuit established: {} -> {}", src_peer_id, dst_peer_id);
                    }
                    BehaviourEvent::Relay(relay::Event::CircuitReqDenied {
                        src_peer_id, ..
                    }) => {
                        warn!("🚫 Circuit request denied from peer: {}", src_peer_id);
                    }
                    BehaviourEvent::Relay(relay::Event::CircuitClosed {
                        src_peer_id,
                        dst_peer_id,
                        ..
                    }) => {
                        info!("🔌 Circuit closed: {} -> {}", src_peer_id, dst_peer_id);
                    }
                    _ => {
                        if cli.verbose {
                            info!("Relay event: {:?}", event);
                        }
                    }
                }
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on: {}", address);
            }
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                info!(
                    "Connection established with peer: {} via {:?}",
                    peer_id, endpoint
                );
            }
            SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                info!("Connection closed with peer: {} - {:?}", peer_id, cause);
            }
            _ => {}
        }
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

fn generate_ed25519(secret_key_seed: u8) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed;

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}
