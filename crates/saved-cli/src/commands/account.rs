//! Account management commands

use crate::utils::formatting::{print_section_header, print_success};
use anyhow::Result;
use colored::*;
use saved_core::{create_or_open_account, Config};
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
        storage_backend: saved_core::storage::StorageBackend::Sqlite,
        account_passphrase: None,
    };

    // Create account as key holder (needed for device linking)
    let account = saved_core::AccountHandle::create_account_key_holder(config).await?;

    print_success("Account created successfully!");
    println!("Account name: {}", name.bright_blue());
    println!(
        "Storage path: {}",
        account_path.display().to_string().bright_blue()
    );

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
        storage_backend: saved_core::storage::StorageBackend::Sqlite,
        account_passphrase: None,
    };

    // Open account
    let account = create_or_open_account(config).await?;

    // Get device info
    let device_info = account.device_info().await;

    print_section_header("SAVED Account Information");
    println!(
        "Storage path: {}",
        account_path.display().to_string().bright_blue()
    );
    println!("Device ID: {}", device_info.device_id.bright_blue());
    println!("Device name: {}", device_info.device_name.bright_blue());
    println!(
        "Last seen: {}",
        device_info
            .last_seen
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string()
            .bright_blue()
    );
    println!(
        "Online: {}",
        if device_info.is_online {
            "Yes".green()
        } else {
            "No".red()
        }
    );

    Ok(())
}
