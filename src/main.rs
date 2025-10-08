mod error;
mod keygen;
mod network;
mod signals;
mod view;

use crate::error::SavedResult;
use crate::keygen::keypair_from_seed;
use crate::network::{KadMode, SavedNetwork};
use env_helpers::get_env_default;
use libp2p::{Multiaddr, PeerId};
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> SavedResult<()> {
    // Get seed phrase from environment variable, panic if missing
    let seed_phrase = get_env_default("SEED_PHRASE", "test".to_string());
    let boostrap = get_env_default("BOOTSTRAP", false);

    println!("Using seed phrase: '{}'", seed_phrase);

    // Generate keypair from seed
    let keypair = keypair_from_seed(&seed_phrase);

    let mut network_handle = SavedNetwork::new(keypair).await?;
    network_handle
        .set_kad_enabled(true, KadMode::Server)
        .await?;
    network_handle.set_mdns_enabled(true).await?;
    sleep(Duration::from_secs(2)).await;
    if boostrap {
        let boostrap_addrs: Vec<Multiaddr> = vec![
            "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
            "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
            "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
            "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
            "/ip4/104.131.131.82/tcp/4001/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ",
            // "/ip4/3.72.112.97/tcp/35076/p2p/QmP2AA8nnXbR2nc3YQvxZtadx1qiFuYnBgw91qsG3zyPX6",
        ]
        .into_iter()
        .map(|s| s.parse().expect("bad multiaddr"))
        .collect();
        for bootstrap_addr in boostrap_addrs {
            network_handle.dial(bootstrap_addr).await?;
        }
        sleep(Duration::from_secs(2)).await;
        network_handle.kad_bootstrap().await?;
    }
    network_handle.kad_find_peer(network_handle.id).await?;

    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = network_handle.events_rx.recv().await;
            println!("{:?}", event);
        }
    })
    .await;

    // SEED 1 and 2 Peers
    network_handle
        .add_target_peer(PeerId::from_str(
            "QmUh4bvMXKXepae9V5pbRwmhd2XqcPHDLrfrAEwpJ6J8os",
        )?)
        .await?;
    network_handle
        .add_target_peer(PeerId::from_str(
            "QmdoTQGmZi2EZKrXagxvL8Uy76ADpS5d53XLvuVXJxyvN1",
        )?)
        .await?;

    sleep(Duration::from_secs(10)).await;

    network_handle.join().await?;

    Ok(())
}
