use thiserror::Error;

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
}

pub type SavedResult<T> = Result<T, SavedError>;
