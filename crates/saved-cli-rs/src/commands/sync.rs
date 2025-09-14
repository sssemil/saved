//! Sync and network commands

use anyhow::Result;
use colored::*;
use saved_core_rs::{create_or_open_account, Config};
use std::path::PathBuf;
use crate::utils::formatting::{print_success, print_section_header};

/// Start network sync
pub async fn sync_command(account_path: &PathBuf, daemon: bool, verbose: bool) -> Result<()> {
    if verbose {
        println!("Starting network sync...");
        println!("Daemon mode: {}", daemon);
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
        storage_backend: saved_core_rs::storage::StorageBackend::Sqlite,
        account_passphrase: None,
    };

    // Open account
    let mut account = create_or_open_account(config).await?;

    // Start network
    account.start_network().await?;

    print_success("Network started successfully!");
    println!("Listening for connections and syncing messages...");

    if daemon {
        println!("Running in daemon mode. Press Ctrl+C to stop.");
        println!("Note: Event handling not yet implemented in core library.");

        // Wait for user interrupt
        tokio::signal::ctrl_c().await?;
        println!("\n{}", "Shutting down...".yellow());
    } else {
        println!(
            "Network started. Use Ctrl+C to stop or run with --daemon for background operation."
        );

        // Wait for user interrupt
        tokio::signal::ctrl_c().await?;
        println!("\n{}", "Shutting down...".yellow());
    }

    Ok(())
}

/// Show sync status
pub async fn status_command(account_path: &PathBuf, verbose: bool) -> Result<()> {
    if verbose {
        println!("Checking sync status...");
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
        storage_backend: saved_core_rs::storage::StorageBackend::Sqlite,
        account_passphrase: None,
    };

    // Open account
    let account = create_or_open_account(config).await?;

    // Get device info
    let device_info = account.device_info().await;

    print_section_header("SAVED Sync Status");

    // Device status
    println!("Device: {}", device_info.device_name.bright_blue());
    println!("ID: {}", device_info.device_id.bright_blue());
    println!(
        "Status: {}",
        if device_info.is_online {
            "Online".green()
        } else {
            "Offline".red()
        }
    );
    println!(
        "Last seen: {}",
        device_info
            .last_seen
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string()
            .bright_blue()
    );

    // Network status
    println!("\n{}", "Network:".bright_blue().bold());
    println!("Status: {}", "Not implemented".yellow());
    println!("Connected peers: {}", "0".bright_blue());
    println!("Sync progress: {}", "N/A".bright_blue());

    // Storage status
    println!("\n{}", "Storage:".bright_blue().bold());
    println!(
        "Account path: {}",
        account_path.display().to_string().bright_blue()
    );
    println!("Messages: {}", "N/A".bright_blue());
    println!("Attachments: {}", "N/A".bright_blue());

    println!("\n{}", "Note:".yellow());
    println!("Detailed sync status is not yet implemented in the core library.");

    Ok(())
}

