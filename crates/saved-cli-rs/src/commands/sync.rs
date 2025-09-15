//! Sync and network commands

use anyhow::Result;
use colored::*;
use saved_core_rs::{create_or_open_account, Config};
use std::path::PathBuf;
use crate::utils::formatting::{print_success, print_section_header};
use comfy_table::{Table, presets::UTF8_FULL, Cell};

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
    let connected_peers = account.connected_peers_count().await;
    println!("Connected peers: {}", connected_peers.to_string().bright_blue());
    let discovered = account.discovered_peers().await;
    println!("Discovered peers: {}", discovered.len().to_string().bright_blue());
    println!("Sync progress: {}", "N/A".bright_blue());

    // Storage status
    println!("\n{}", "Storage:".bright_blue().bold());
    println!(
        "Account path: {}",
        account_path.display().to_string().bright_blue()
    );
    if let Ok(stats) = account.storage_stats().await {
        println!("Messages: {}", stats.message_count.to_string().bright_blue());
        println!("Operations: {}", stats.operation_count.to_string().bright_blue());
        println!("Chunks: {}", stats.chunk_count.to_string().bright_blue());
        println!("Total size: {} bytes", stats.total_size.to_string().bright_blue());
    } else {
        println!("Messages: {}", "N/A".bright_blue());
        println!("Attachments: {}", "N/A".bright_blue());
    }

    println!("\n{}", "Note:".yellow());
    println!("Detailed sync status is not yet implemented in the core library.");

    Ok(())
}

/// Discover peers (mDNS/manual scan) and print results
pub async fn discover_command(account_path: &PathBuf, verbose: bool) -> Result<()> {
    if verbose {
        println!("Starting discovery...");
    }

    let config = Config {
        storage_path: account_path.clone(),
        network_port: 0,
        enable_mdns: true,
        allow_public_relays: false,
        bootstrap_multiaddrs: Vec::new(),
        use_kademlia: false,
        chunk_size: 2 * 1024 * 1024,
        max_parallel_chunks: 4,
        storage_backend: saved_core_rs::storage::StorageBackend::Sqlite,
        account_passphrase: None,
    };

    let mut account = create_or_open_account(config).await?;
    account.start_network().await?;

    // Optionally perform a manual local network scan to augment mDNS
    let _ = account.scan_local_network().await;
    // Give discovery some time to run
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let discovered = account.discovered_peers().await;

    print_section_header("Discovered Peers:");
    if discovered.is_empty() {
        println!("{}", "No peers discovered".yellow());
    } else {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(vec!["Device ID", "Addresses", "Method", "Last Seen"]);
        for (_id, info) in discovered.iter() {
            table.add_row(vec![
                Cell::new(&info.device_id),
                Cell::new(info.addresses.join(", ")),
                Cell::new(format!("{:?}", info.discovery_method)),
                Cell::new(info.last_seen.format("%Y-%m-%d %H:%M:%S").to_string()),
            ]);
        }
        println!("{}", table);
    }

    Ok(())
}

/// Connect to a discovered peer by device id
pub async fn connect_command(account_path: &PathBuf, device_id: &str, override_addresses: Vec<String>, verbose: bool) -> Result<()> {
    if verbose {
        println!("Connecting to peer {}...", device_id);
    }

    let config = Config {
        storage_path: account_path.clone(),
        network_port: 0,
        enable_mdns: true,
        allow_public_relays: false,
        bootstrap_multiaddrs: Vec::new(),
        use_kademlia: false,
        chunk_size: 2 * 1024 * 1024,
        max_parallel_chunks: 4,
        storage_backend: saved_core_rs::storage::StorageBackend::Sqlite,
        account_passphrase: None,
    };

    let mut account = create_or_open_account(config).await?;
    account.start_network().await?;

    // Determine addresses
    let addresses = if !override_addresses.is_empty() {
        override_addresses
    } else {
        let discovered = account.discovered_peers().await;
        if let Some(info) = discovered.get(device_id) {
            info.addresses.clone()
        } else {
            Vec::new()
        }
    };

    if addresses.is_empty() {
        println!("{}", "No addresses available to connect".yellow());
        return Ok(());
    }

    match account.connect_to_peer(device_id.to_string(), addresses.clone()).await {
        Ok(_) => {
            print_success("Connection attempt started/succeeded");
            if verbose {
                println!("Addresses: {}", addresses.join(", "));
            }
        }
        Err(e) => {
            println!("{} {}", "Failed to connect:".red(), e);
        }
    }

    Ok(())
}

