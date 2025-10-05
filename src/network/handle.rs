use crate::network::{KadMode, SavedNetworkCommand, SavedNetworkEvent};
use libp2p::{Multiaddr, PeerId};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

pub struct SavedHandle {
    pub id: PeerId,
    pub events_rx: broadcast::Receiver<SavedNetworkEvent>,
    pub(crate) cmd_tx: mpsc::Sender<SavedNetworkCommand>,
    pub(crate) task_handle: Option<JoinHandle<()>>,
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

    pub async fn set_kad_enabled(
        &self,
        enabled: bool,
        mode: KadMode,
    ) -> Result<(), mpsc::error::SendError<SavedNetworkCommand>> {
        self.cmd_tx
            .send(SavedNetworkCommand::SetKadEnabled(enabled, mode))
            .await
    }
    pub async fn kad_find_peer(
        &self,
        target: PeerId,
    ) -> Result<(), mpsc::error::SendError<SavedNetworkCommand>> {
        self.cmd_tx
            .send(SavedNetworkCommand::KadFindPeer(target))
            .await
    }

    pub async fn kad_bootstrap(&self) -> Result<(), mpsc::error::SendError<SavedNetworkCommand>> {
        self.cmd_tx.send(SavedNetworkCommand::KadBootstrap).await
    }

    pub async fn dial_peer(
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
