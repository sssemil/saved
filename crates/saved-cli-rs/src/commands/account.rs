//! Account management commands

use anyhow::Result;
use colored::*;
use saved_core_rs::{create_or_open_account, Config};
use std::path::PathBuf;

/// Initialize a new SAVED account
pub async fn init_command(account_path: &PathBuf, name: &str, verbose: bool) -> Result<()> {
    if verbose {
        println!("Initializing new SAVED account...");
        println!("Account name: {}", name);
        println!("Storage path: {}", account_path.display());
    }

    // Create configuration
    let config = Config {
        storage_path: account_path.clone(),
        network_port: 8080,
        enable_mdns: true,
        allow_public_relays: false,
        bootstrap_multiaddrs: Vec::new(),
        use_kademlia: false,
        chunk_size: 2 * 1024 * 1024, // 2 MiB
        max_parallel_chunks: 4,
    };

    // Create account
    let account = create_or_open_account(config).await?;
    
    println!("{}", "✓ Account created successfully!".green().bold());
    println!("Account name: {}", name.bright_blue());
    println!("Storage path: {}", account_path.display().to_string().bright_blue());
    
    // Show device info
    let device_info = account.device_info().await;
    println!("Device ID: {}", device_info.device_id.bright_blue());
    println!("Device name: {}", device_info.device_name.bright_blue());
    
    println!("\n{}", "Next steps:".yellow().bold());
    println!("• Use 'saved create --content \"Hello, SAVED!\"' to create your first message");
    println!("• Use 'saved link' to generate a QR code for linking other devices");
    println!("• Use 'saved sync' to start syncing with other devices");

    Ok(())
}

/// Show account information
pub async fn info_command(account_path: &PathBuf, verbose: bool) -> Result<()> {
    if verbose {
        println!("Loading account information...");
    }

    // Create configuration
    let config = Config {
        storage_path: account_path.clone(),
        network_port: 8080,
        enable_mdns: true,
        allow_public_relays: false,
        bootstrap_multiaddrs: Vec::new(),
        use_kademlia: false,
        chunk_size: 2 * 1024 * 1024, // 2 MiB
        max_parallel_chunks: 4,
    };

    // Open account
    let account = create_or_open_account(config).await?;
    
    // Get device info
    let device_info = account.device_info().await;
    
    println!("{}", "SAVED Account Information".bright_blue().bold());
    println!("{}", "=".repeat(25).bright_blue());
    println!("Storage path: {}", account_path.display().to_string().bright_blue());
    println!("Device ID: {}", device_info.device_id.bright_blue());
    println!("Device name: {}", device_info.device_name.bright_blue());
    println!("Last seen: {}", device_info.last_seen.format("%Y-%m-%d %H:%M:%S UTC").to_string().bright_blue());
    println!("Online: {}", if device_info.is_online { "Yes".green() } else { "No".red() });

    Ok(())
}
