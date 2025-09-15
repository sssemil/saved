//! SAVED Daemon
//!
//! Background daemon for SAVED network management, storage, and peer connections.
//! Similar to Tailscale's daemon architecture.

use anyhow::Result;
use clap::Parser;
use saved_core::{AccountHandle, Config, storage::StorageBackend};
use std::path::PathBuf;
use tokio::signal;
use tracing::{info, error};

#[derive(Parser)]
#[command(name = "saved-daemon")]
#[command(about = "SAVED daemon for background network management")]
struct Args {
    /// Path to the account storage directory
    #[arg(long, default_value = "./saved-account")]
    account_path: PathBuf,
    
    /// Network port (0 for random)
    #[arg(long, default_value = "0")]
    network_port: u16,
    
    /// Enable mDNS discovery
    #[arg(long, default_value = "true")]
    enable_mdns: bool,
    
    /// Allow public relays
    #[arg(long, default_value = "false")]
    allow_public_relays: bool,
    
    /// Account passphrase (optional)
    #[arg(long)]
    passphrase: Option<String>,
    
    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    
    info!("Starting SAVED daemon...");
    info!("Account path: {}", args.account_path.display());
    info!("Network port: {}", args.network_port);
    
    // Create account configuration
    let config = Config {
        storage_path: args.account_path.clone(),
        network_port: args.network_port,
        enable_mdns: args.enable_mdns,
        allow_public_relays: args.allow_public_relays,
        bootstrap_multiaddrs: Vec::new(),
        use_kademlia: false,
        chunk_size: 2 * 1024 * 1024, // 2 MiB
        max_parallel_chunks: 4,
        storage_backend: StorageBackend::Sqlite,
        account_passphrase: args.passphrase,
    };
    
    // Create or open account
    let mut account = if config.storage_path.exists() {
        info!("Loading existing account...");
        AccountHandle::create_or_open(config).await?
    } else {
        info!("Creating new account...");
        AccountHandle::create_account_key_holder(config).await?
    };
    
    // Start network
    info!("Starting network...");
    account.start_network().await?;
    
    // Get device info
    let device_info = account.device_info().await;
    info!("Device ID: {}", device_info.device_id);
    info!("Device name: {}", device_info.device_name);
    info!("Authorized: {}", device_info.is_authorized);
    
    info!("SAVED daemon started successfully!");
    info!("Use 'savedctl status' to check daemon status");
    info!("Use 'savedctl peer list' to see connected peers");
    
    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("Received shutdown signal, stopping daemon...");
        }
        Err(err) => {
            error!("Failed to listen for shutdown signal: {}", err);
        }
    }
    
    info!("SAVED daemon stopped");
    Ok(())
}
