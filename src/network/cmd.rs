use crate::error::SavedResult;
use crate::network::SavedNetwork;
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};

#[derive(Debug, Clone)]
pub enum SavedNetworkCommand {
    SetMdnsEnabled(bool),
    SetKadEnabled(bool, KadMode),
    KadFindPeer(PeerId),
    KadBootstrap,
    Dial(Multiaddr),
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

impl SavedNetwork {
    pub(crate) async fn on_handle_cmd(&mut self, cmd: SavedNetworkCommand) -> SavedResult<()> {
        println!("Received a command: {:?}", cmd);
        match cmd {
            SavedNetworkCommand::SetMdnsEnabled(enabled) => {
                self.set_mdns_enabled(enabled).await;
            }
            SavedNetworkCommand::SetKadEnabled(enabled, mode) => {
                self.set_kad_enabled(enabled, mode).await;
            }
            SavedNetworkCommand::KadFindPeer(target) => {
                if let Some(kad) = self.swarm.behaviour_mut().kad.as_mut() {
                    kad.get_closest_peers(target);
                } else {
                    eprintln!("kad find requested but Kademlia is disabled");
                }
            }
            SavedNetworkCommand::KadBootstrap => {
                if let Some(kad) = self.swarm.behaviour_mut().kad.as_mut() {
                    kad.bootstrap()?;
                } else {
                    eprintln!("kad bootstrap requested but Kademlia is disabled");
                }
            }
            SavedNetworkCommand::Dial(addr) => {
                // Ensure the multiaddr has /p2p/<target>
                let has_p2p = addr.iter().any(|p| matches!(p, Protocol::P2p(_)));
                if !has_p2p {
                    eprintln!("dial addr doesn't have p2p protocol: {:?}", addr);
                } else {
                    match self.swarm.dial(addr) {
                        Ok(()) => {}
                        Err(e) => eprintln!("dial failed: {e}"),
                    }
                }
            }
        }
        Ok(())
    }
}
