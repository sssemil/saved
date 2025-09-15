//! Device linking commands

use anyhow::Result;
use colored::*;
use qrcode::QrCode;
use saved_core_rs::{create_or_open_account, Config, QrPayload};
use std::path::PathBuf;
use crate::utils::formatting::{print_success, print_section_header};
use comfy_table::{Table, presets::UTF8_FULL, Cell};

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
        account_passphrase: None,
    };

    // Open account
    let account = create_or_open_account(config).await?;

    // Generate QR payload
    let qr_payload = account.make_linking_qr().await?;

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

    print_section_header("Device Linking QR Code:");
    println!("{}", string);

    // Save to file if requested
    if let Some(output_path) = output {
        // Implement proper device export with multiple formats
        let extension = output_path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt");
        
        match extension.to_lowercase().as_str() {
            "json" => {
                // Save as JSON payload
                std::fs::write(&output_path, &json_payload)?;
                println!(
                    "QR payload saved to: {}",
                    output_path.display().to_string().bright_blue()
                );
            },
            "txt" => {
                // Save as formatted text file
                let mut content = String::new();
                content.push_str("SAVED Device Linking Information\n");
                content.push_str("================================\n\n");
                content.push_str("QR Code:\n");
                content.push_str(&string);
                content.push_str("\n\nJSON Payload:\n");
                content.push_str(&json_payload);
                content.push_str("\n\nInstructions:\n");
                content.push_str("1. Open SAVED on another device\n");
                content.push_str("2. Scan this QR code with the 'saved accept' command\n");
                content.push_str("3. The devices will be linked and can sync messages\n");
                
                std::fs::write(&output_path, content)?;
                println!(
                    "Device export saved to: {}",
                    output_path.display().to_string().bright_blue()
                );
            },
            "svg" => {
                // Generate SVG QR code (simplified)
                let qr = QrCode::new(&json_payload)?;
                let svg = qr.render::<qrcode::render::svg::Color>().build();
                
                std::fs::write(&output_path, svg)?;
                println!(
                    "SVG QR code saved to: {}",
                    output_path.display().to_string().bright_blue()
                );
            },
            _ => {
                // Default to text format
                let mut content = String::new();
                content.push_str("SAVED Device Linking Information\n");
                content.push_str("================================\n\n");
                content.push_str("QR Code:\n");
                content.push_str(&string);
                content.push_str("\n\nJSON Payload:\n");
                content.push_str(&json_payload);
                
                std::fs::write(&output_path, content)?;
                println!(
                    "Device export saved to: {}",
                    output_path.display().to_string().bright_blue()
                );
            }
        }
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
        account_passphrase: None,
    };

    // Open account
    let account = create_or_open_account(config).await?;

    // Accept the link
    let device_info = account.accept_link(qr_payload).await?;

    print_success("Device linked successfully!");
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
        account_passphrase: None,
    };

    // Open account
    let account = create_or_open_account(config).await?;

    // Get device info
    let device_info = account.device_info().await;

    print_section_header("Devices:");

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Type", "Device ID", "Name", "State", "Health", "Addresses"]);

    // Local device row
    table.add_row(vec![
        Cell::new("Local"),
        Cell::new(&device_info.device_id),
        Cell::new(&device_info.device_name),
        Cell::new("Online"),
        Cell::new("Healthy"),
        Cell::new("-"),
    ]);

    // Connected peers
    let connected = account.connected_peers().await;
    for (id, info) in connected.iter() {
        let state = account.peer_connection_state(id).await.unwrap_or_else(|| "Connected".to_string());
        let health = account.peer_health(id).await.unwrap_or_else(|| "Unknown".to_string());
        table.add_row(vec![
            Cell::new("Connected"),
            Cell::new(id),
            Cell::new(&info.device_name),
            Cell::new(state),
            Cell::new(health),
            Cell::new("-"),
        ]);
    }

    // Discovered peers (not necessarily connected)
    let discovered = account.discovered_peers().await;
    for (id, di) in discovered.iter() {
        // Skip if already connected and shown
        if connected.contains_key(id) { continue; }
        table.add_row(vec![
            Cell::new("Discovered"),
            Cell::new(id),
            Cell::new("-"),
            Cell::new("Disconnected"),
            Cell::new("Unknown"),
            Cell::new(di.addresses.join(", ")),
        ]);
    }

    println!("{}", table);

    Ok(())
}
