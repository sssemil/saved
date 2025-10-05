use crate::error::SavedResult;
use crate::network::{KadMode, SavedHandle, SavedNetwork};
use crate::saved_rpc;
use libp2p::{Multiaddr, PeerId};

saved_rpc! {
    trait SavedNetworkApi for SavedNetwork {
        async fn set_mdns_enabled(enabled: bool) -> SavedResult<()>;
        async fn set_kad_enabled(enabled: bool, mode: KadMode) -> SavedResult<()>;
        async fn kad_find_peer(target: PeerId) -> SavedResult<()>;
        async fn kad_bootstrap() -> SavedResult<()>;
        async fn dial(addr: Multiaddr) -> SavedResult<()>;
    }

    handle = SavedHandle,
    cmd_enum = SavedNetworkRpc,

    result = SavedResult<()>,
    error  = crate::error::SavedError
}
