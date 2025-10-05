use crate::network;
use libp2p::Multiaddr;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum SavedError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Libp2p Noise Error: {0}")]
    Libp2pNoise(#[from] libp2p::noise::Error),
    #[error("Libp2p Multiaddress Error: {0}")]
    Libp2pMultiaddress(#[from] libp2p::multiaddr::Error),
    #[error("Libp2p Transport Error: {0}")]
    Libp2pCoreTransportError(#[from] libp2p::TransportError<std::io::Error>),
    #[error("Infallible: {0}")]
    Infallible(#[from] std::convert::Infallible),
    #[error("Join Error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("tokio::sync::broadcast::error::SendError<network::SavedNetworkEvent>: {0}")]
    TokioBroadcastNetEvent(
        #[from] tokio::sync::broadcast::error::SendError<network::SavedNetworkEvent>,
    ),
    #[error("Kad NoKnownPeers: {0}")]
    KadNoKnownPeers(#[from] libp2p::kad::NoKnownPeers),
    #[error("Kad Disabled")]
    KadDisabled,
    #[error("dial addr missing /p2p/<peerid>: {0}")]
    DialAddrMissingP2p(Multiaddr),
    #[error("DialError: {0}")]
    Dial(#[from] libp2p::swarm::DialError),
    #[error("tokio::sync::mpsc::error::SendError<network::rpc::SavedNetworkRpc>: {0}")]
    TokioMpscSendNetRpc(#[from] mpsc::error::SendError<network::api::SavedNetworkRpc>),
    #[error("oneshot canceled while awaiting RPC reply for {rpc}")]
    OneshotCanceled { rpc: &'static str },
}

pub type SavedResult<T> = Result<T, SavedError>;
