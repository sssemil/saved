mod error;
mod keygen;
mod network;
mod signals;
mod view;

use crate::error::SavedResult;
use crate::keygen::keypair_from_seed;
use crate::network::SavedNetwork;
use env_helpers::get_env_default;

#[tokio::main]
async fn main() -> SavedResult<()> {
    // Get seed phrase from environment variable, panic if missing
    let seed_phrase = get_env_default("SEED_PHRASE", "test".to_string());

    println!("Using seed phrase: '{}'", seed_phrase);

    // Generate keypair from seed
    let keypair = keypair_from_seed(&seed_phrase);

    let network_handle = SavedNetwork::new(keypair).await?;
    network_handle.join().await;

    Ok(())
}
