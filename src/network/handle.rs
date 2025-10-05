use crate::error::SavedResult;
use crate::network::SavedNetworkEvent;
use crate::network::api::SavedNetworkRpc;
use libp2p::PeerId;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

pub struct SavedHandle {
    pub id: PeerId,
    pub events_rx: broadcast::Receiver<SavedNetworkEvent>,
    pub(crate) cmd_tx: mpsc::Sender<SavedNetworkRpc>,
    pub(crate) task_handle: Option<JoinHandle<()>>,
}

impl SavedHandle {
    pub async fn join(mut self) -> SavedResult<()> {
        if let Some(handle) = self.task_handle.take() {
            // Await consumes the JoinHandle; after take() we own it.
            handle.await?;
        }
        Ok(())
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
