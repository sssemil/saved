mod env;
mod error;
mod keygen;
mod network;
mod signals;
mod view;

use crate::env::get_env_default;
use crate::error::SavedResult;
use crate::keygen::keypair_from_seed;
use crate::network::SavedNetwork;
use tokio::join;

#[tokio::main]
async fn main() -> SavedResult<()> {
    // Get seed phrase from environment variable, panic if missing
    let seed_phrase = get_env_default("SEED_PHRASE", "test".to_string());

    println!("Using seed phrase: '{}'", seed_phrase);

    // Generate keypair from seed
    let keypair = keypair_from_seed(&seed_phrase);

    let network = SavedNetwork::new(keypair)?;

    println!("Starting Swarm server...");
    let h = network.run().await;
    let (r,) = join!(h);
    r?;

    Ok(())
}
