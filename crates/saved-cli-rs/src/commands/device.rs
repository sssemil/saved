//! Device linking commands

use anyhow::Result;
use colored::*;
use qrcode::QrCode;
use saved_core_rs::{create_or_open_account, Config, QrPayload};
use std::path::PathBuf;

/// Generate QR code for device linking
pub async fn link_command(
    account_path: &PathBuf,
    output: Option<PathBuf>,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Generating QR code for device linking...");
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
    };

    // Open account
    let account = create_or_open_account(config).await?;

    // Generate QR payload
    let qr_payload = account.make_linking_qr();

    // Serialize to JSON
    let json_payload = serde_json::to_string_pretty(&qr_payload)?;

    if verbose {
        println!("QR Payload: {}", json_payload);
    }

    // Generate QR code
    let code = QrCode::new(&json_payload)?;
    let string = code
        .render::<char>()
        .quiet_zone(false)
        .module_dimensions(2, 1)
        .build();

    println!("{}", "Device Linking QR Code:".bright_blue().bold());
    println!("{}", "=".repeat(25).bright_blue());
    println!("{}", string);

    // Save to file if requested
    if let Some(output_path) = output {
        // For now, just save the JSON payload to a text file
        std::fs::write(&output_path, &json_payload)?;
        println!(
            "QR payload saved to: {}",
            output_path.display().to_string().bright_blue()
        );
        println!("Note: Image generation not yet implemented. Saved JSON payload instead.");
    }

    println!("\n{}", "Instructions:".yellow().bold());
    println!("1. Open SAVED on another device");
    println!("2. Scan this QR code with the 'saved accept' command");
    println!("3. The devices will be linked and can sync messages");

    Ok(())
}

/// Accept device link from QR code
pub async fn accept_command(account_path: &PathBuf, payload: &str, verbose: bool) -> Result<()> {
    if verbose {
        println!("Accepting device link...");
        println!("Payload: {}", payload);
    }

    // Parse QR payload
    let qr_payload: QrPayload = serde_json::from_str(payload)?;

    if verbose {
        println!("Device ID: {}", qr_payload.device_id);
        println!("Addresses: {:?}", qr_payload.addresses);
        println!("Expires at: {}", qr_payload.expires_at);
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
    };

    // Open account
    let account = create_or_open_account(config).await?;

    // Accept the link
    let device_info = account.accept_link(qr_payload).await?;

    println!("{}", "✓ Device linked successfully!".green().bold());
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

    println!("\n{}", "Next steps:".yellow().bold());
    println!("• Use 'saved sync' to start syncing messages");
    println!("• Use 'saved devices' to see all connected devices");

    Ok(())
}

/// Show connected devices
pub async fn devices_command(account_path: &PathBuf, verbose: bool) -> Result<()> {
    if verbose {
        println!("Listing connected devices...");
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
    };

    // Open account
    let account = create_or_open_account(config).await?;

    // Get device info
    let device_info = account.device_info().await;

    println!("{}", "Connected Devices:".bright_blue().bold());
    println!("{}", "=".repeat(20).bright_blue());

    // Show local device
    println!("• {} (Local)", device_info.device_name.bright_green());
    println!("  ID: {}", device_info.device_id.bright_blue());
    println!(
        "  Status: {}",
        if device_info.is_online {
            "Online".green()
        } else {
            "Offline".red()
        }
    );
    println!(
        "  Last seen: {}",
        device_info
            .last_seen
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string()
            .bright_blue()
    );

    println!("\n{}", "Note:".yellow());
    println!(
        "Device discovery and connection management is not yet implemented in the core library."
    );

    Ok(())
}
