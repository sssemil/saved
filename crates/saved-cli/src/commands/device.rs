//! Device linking commands

use anyhow::Result;
use colored::*;
use saved_core::{create_or_open_account, Config, QrPayload};
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
        storage_backend: saved_core::storage::StorageBackend::Sqlite,
        account_passphrase: None,
    };

    // Open account
    let mut account = create_or_open_account(config).await?;

    // Start network to get current addresses
    account.start_network().await?;

    // Generate QR payload
    let qr_payload = account.make_linking_qr().await?;

    // Serialize to JSON
    let json_payload = serde_json::to_string_pretty(&qr_payload)?;

    if verbose {
        println!("QR Payload: {}", json_payload);
        println!("Device ID: {}", qr_payload.device_id);
        println!("Expires at: {}", qr_payload.expires_at);
        println!("Network addresses: {:?}", qr_payload.addresses);
    }

    // QR code generation removed for now - too large for CLI display

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
                content.push_str("JSON Payload:\n");
                content.push_str(&json_payload);
                content.push_str("\n\nInstructions:\n");
                content.push_str("1. Open SAVED on another device\n");
                content.push_str("2. Use the 'saved accept' command with this JSON payload\n");
                content.push_str("3. The devices will be linked and can sync messages\n");
                
                std::fs::write(&output_path, content)?;
                println!(
                    "Device export saved to: {}",
                    output_path.display().to_string().bright_blue()
                );
            },
            _ => {
                // Default to text format
                let mut content = String::new();
                content.push_str("SAVED Device Linking Information\n");
                content.push_str("================================\n\n");
                content.push_str("JSON Payload:\n");
                content.push_str(&json_payload);
                content.push_str("\n\nInstructions:\n");
                content.push_str("1. Open SAVED on another device\n");
                content.push_str("2. Use the 'saved accept' command with this JSON payload\n");
                content.push_str("3. The devices will be linked and can sync messages\n");
                
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
    println!("2. Use the 'saved accept' command with the JSON payload");
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
        storage_backend: saved_core::storage::StorageBackend::Sqlite,
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
        storage_backend: saved_core::storage::StorageBackend::Sqlite,
        account_passphrase: None,
    };

    // Open account and start network
    let mut account = create_or_open_account(config).await?;
    account.start_network().await?;

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

/// Accept a device link from a QR code payload
pub async fn accept_link_command(
    account_path: &PathBuf,
    qr_payload_json: String,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Accepting device link from QR code payload...");
    }

    // Parse the QR payload
    let qr_payload: saved_core::QrPayload = serde_json::from_str(&qr_payload_json)
        .map_err(|e| anyhow::anyhow!("Failed to parse QR payload: {}", e))?;

    if verbose {
        println!("QR Payload parsed successfully:");
        println!("  Device ID: {}", qr_payload.device_id);
        println!("  Expires at: {}", qr_payload.expires_at);
        println!("  Addresses: {:?}", qr_payload.addresses);
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

    // Accept the device link
    match account.accept_link(qr_payload).await {
        Ok(device_info) => {
            print_success("Device link accepted successfully!");
            println!("Device ID: {}", device_info.device_id);
            println!("Device Name: {}", device_info.device_name);
            println!("Authorized: {}", if device_info.is_authorized { "Yes" } else { "No" });
            
            if device_info.is_authorized {
                println!("✅ Device is authorized and can sync messages");
            } else {
                println!("⚠️  Device is not authorized - certificate verification failed");
            }
        }
        Err(e) => {
            println!("{} {}", "Failed to accept device link:".red(), e);
            return Err(anyhow::anyhow!("Device link failed: {}", e));
        }
    }

    Ok(())
}

/// List all authorized devices
pub async fn list_authorized_command(
    account_path: &PathBuf,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Listing authorized devices...");
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

    // List authorized devices
    match account.list_authorized_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                println!("No authorized devices found.");
            } else {
                print_section_header("Authorized Devices:");
                
                let mut table = Table::new();
                table.load_preset(UTF8_FULL);
                table.set_header(vec!["Device ID", "Name", "Last Seen", "Certificate Expires"]);
                
                for device in devices {
                    let expires_str = if let Some(cert) = &device.device_cert {
                        if let Some(expires) = cert.expires_at {
                            expires.format("%Y-%m-%d %H:%M:%S UTC").to_string()
                        } else {
                            "Never".to_string()
                        }
                    } else {
                        "Unknown".to_string()
                    };
                    
                    table.add_row(vec![
                        Cell::new(&device.device_id),
                        Cell::new(&device.device_name),
                        Cell::new(device.last_seen.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
                        Cell::new(expires_str),
                    ]);
                }
                
                println!("{}", table);
            }
        }
        Err(e) => {
            println!("{} {}", "Failed to list authorized devices:".red(), e);
            return Err(anyhow::anyhow!("List devices failed: {}", e));
        }
    }

    Ok(())
}

/// Revoke device authorization
pub async fn revoke_device_command(
    account_path: &PathBuf,
    device_id: String,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Revoking device authorization for: {}", device_id);
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
    let mut account = create_or_open_account(config).await?;

    // Revoke device
    match account.revoke_device(&device_id).await {
        Ok(_) => {
            print_success(&format!("Device {} authorization revoked successfully!", device_id));
        }
        Err(e) => {
            println!("{} {}", "Failed to revoke device authorization:".red(), e);
            return Err(anyhow::anyhow!("Revoke device failed: {}", e));
        }
    }

    Ok(())
}
