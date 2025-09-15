//! SAVED Control Tool
//!
//! CLI tool for controlling the SAVED daemon, similar to tailscale CLI.

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use saved_core::{storage::StorageBackend, AccountHandle, Config};
use std::path::PathBuf;

mod client;
use client::{DaemonClient, DaemonRequest, DaemonResponse};

fn load_control_port(account_path: &PathBuf) -> Result<Option<u16>> {
    let port_file = account_path.join("control.port");

    if !port_file.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(port_file)?;
    let port: u16 = content.trim().parse()?;
    Ok(Some(port))
}

#[derive(Parser)]
#[command(name = "savedctl")]
#[command(about = "SAVED control tool")]
#[command(version)]
struct Cli {
    /// Path to the account storage directory
    #[arg(long, default_value = "./saved-account")]
    account_path: PathBuf,

    /// Account passphrase (optional)
    #[arg(long)]
    passphrase: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show daemon status and network information
    Status,
    /// Manage devices
    Device {
        #[command(subcommand)]
        command: DeviceCommands,
    },
    /// Manage peers and connections
    Peer {
        #[command(subcommand)]
        command: PeerCommands,
    },
    /// Manage messages
    Message {
        #[command(subcommand)]
        command: MessageCommands,
    },
    /// Manage attachments
    Attachment {
        #[command(subcommand)]
        command: AttachmentCommands,
    },
    /// Manage network
    Network {
        #[command(subcommand)]
        command: NetworkCommands,
    },
    /// Manage account
    Account {
        #[command(subcommand)]
        command: AccountCommands,
    },
    /// Manage chunk synchronization
    Chunk {
        #[command(subcommand)]
        command: ChunkCommands,
    },
}

#[derive(Subcommand)]
enum DeviceCommands {
    /// List all devices
    List,
    /// Show device information
    Info {
        /// Device ID
        device_id: String,
    },
    /// Generate QR code for device linking
    Link,
    /// Accept device link from QR code
    Accept {
        /// QR payload string
        qr_payload: String,
    },
    /// Revoke device authorization
    Revoke {
        /// Device ID to revoke
        device_id: String,
    },
}

#[derive(Subcommand)]
enum PeerCommands {
    /// List all peers (connected and discovered)
    List,
    /// Connect to a peer
    Connect {
        /// Peer device ID
        device_id: String,
        /// Peer addresses (comma-separated)
        addresses: Option<String>,
    },
    /// Disconnect from a peer
    Disconnect {
        /// Peer device ID
        device_id: String,
    },
    /// Scan for peers
    Scan,
}

#[derive(Subcommand)]
enum MessageCommands {
    /// List all messages
    List,
    /// Send a message
    Send {
        /// Message content
        content: String,
        /// Attachment file paths (comma-separated)
        attachments: Option<String>,
    },
    /// Edit a message
    Edit {
        /// Message ID
        message_id: String,
        /// New message content
        new_content: String,
    },
    /// Delete a message (soft delete)
    Delete {
        /// Message ID
        message_id: String,
    },
    /// Purge a message (hard delete)
    Purge {
        /// Message ID
        message_id: String,
    },
}

#[derive(Subcommand)]
enum NetworkCommands {
    /// Show network status
    Status,
    /// Show listening addresses
    Addresses,
    /// Start network discovery
    Start,
    /// Stop network discovery
    Stop,
    /// Scan local network for peers
    Scan,
}

#[derive(Subcommand)]
enum AttachmentCommands {
    /// List all attachments
    List,
    /// Download an attachment
    Download {
        /// Attachment ID
        attachment_id: i64,
        /// Output file path
        output_path: String,
    },
    /// Delete an attachment (soft delete)
    Delete {
        /// Attachment ID
        attachment_id: i64,
    },
    /// Purge an attachment (hard delete)
    Purge {
        /// Attachment ID
        attachment_id: i64,
    },
}

#[derive(Subcommand)]
enum AccountCommands {
    /// Export account data
    Export {
        /// Output file path
        output_path: String,
    },
    /// Import account data
    Import {
        /// Input file path
        input_path: String,
    },
}

#[derive(Subcommand)]
enum ChunkCommands {
    /// Initialize chunk synchronization
    Init,
    /// Store a chunk
    Store {
        /// File path to store as chunk
        file_path: String,
    },
    /// Get a chunk by ID
    Get {
        /// Chunk ID (64 hex characters)
        chunk_id: String,
        /// Output file path
        output_path: String,
    },
    /// Check chunk availability
    Check {
        /// Chunk IDs to check (64 hex characters each)
        chunk_ids: Vec<String>,
    },
    /// Fetch missing chunks
    Fetch {
        /// Chunk IDs to fetch (64 hex characters each)
        chunk_ids: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Create account configuration
    let config = Config {
        storage_path: cli.account_path.clone(),
        network_port: 0, // Not used for read-only operations
        enable_mdns: true,
        allow_public_relays: false,
        bootstrap_multiaddrs: Vec::new(),
        use_kademlia: false,
        chunk_size: 2 * 1024 * 1024,
        max_parallel_chunks: 4,
        storage_backend: StorageBackend::Sqlite,
        account_passphrase: cli.passphrase,
    };

    // Try to connect to daemon first
    let control_port = load_control_port(&cli.account_path)?;

    if let Some(port) = control_port {
        let client = DaemonClient::new(port);
        if client.is_daemon_running().await {
            // Use daemon communication
            match cli.command {
                Commands::Status => handle_status_via_daemon(&client).await?,
                Commands::Device { command } => handle_device_via_daemon(&client, command).await?,
                Commands::Peer { command } => handle_peer_via_daemon(&client, command).await?,
                Commands::Message { command } => {
                    handle_message_via_daemon(&client, command).await?
                }
                Commands::Attachment { command } => {
                    handle_attachment_via_daemon(&client, command).await?
                }
                Commands::Network { command } => {
                    handle_network_via_daemon(&client, command).await?
                }
                Commands::Account { command } => {
                    handle_account_via_daemon(&client, command).await?
                }
                Commands::Chunk { command } => handle_chunk_via_daemon(&client, command).await?,
            }
            return Ok(());
        }
    }

    // Fallback to direct database access if daemon not running
    println!(
        "{}",
        "Daemon not running, using direct database access".yellow()
    );
    let mut account = AccountHandle::create_or_open(config).await?;

    match cli.command {
        Commands::Status => handle_status(&account).await?,
        Commands::Device { command } => handle_device(&account, command).await?,
        Commands::Peer { command } => handle_peer(&account, command).await?,
        Commands::Message { command } => handle_message(&mut account, command).await?,
        Commands::Attachment { command } => handle_attachment(&account, command).await?,
        Commands::Network { command } => handle_network(&account, command).await?,
        Commands::Account { command } => handle_account(&account, command).await?,
        Commands::Chunk { command } => handle_chunk(&mut account, command).await?,
    }

    Ok(())
}

async fn handle_status(account: &AccountHandle) -> Result<()> {
    println!("{}", "SAVED Status".bright_blue().bold());
    println!("=============");

    // Device info
    let device_info = account.device_info().await;
    println!("\n{}", "Device Information".yellow().bold());
    println!("  Device ID: {}", device_info.device_id.bright_blue());
    println!("  Device Name: {}", device_info.device_name.bright_blue());
    println!(
        "  Authorized: {}",
        if device_info.is_authorized {
            "Yes".green()
        } else {
            "No".red()
        }
    );
    println!(
        "  Last Seen: {}",
        format!("{}", device_info.last_seen.format("%Y-%m-%d %H:%M:%S UTC")).bright_blue()
    );

    // Network status
    println!("\n{}", "Network Status".yellow().bold());
    let connected_count = account.connected_peers_count().await;
    let discovered_peers = account.discovered_peers().await;
    let discovered_count = discovered_peers.len();

    println!(
        "  Connected Peers: {}",
        connected_count.to_string().bright_blue()
    );
    println!(
        "  Discovered Peers: {}",
        discovered_count.to_string().bright_blue()
    );
    println!(
        "  Network Status: {}",
        if connected_count > 0 || discovered_count > 0 {
            "Active".green()
        } else {
            "Inactive".red()
        }
    );

    // Messages
    let messages = account.list_messages().await?;
    println!("\n{}", "Messages".yellow().bold());
    println!(
        "  Total Messages: {}",
        messages.len().to_string().bright_blue()
    );

    // Devices
    let devices = account.list_authorized_devices().await?;
    println!("\n{}", "Authorized Devices".yellow().bold());
    println!(
        "  Total Devices: {}",
        devices.len().to_string().bright_blue()
    );

    Ok(())
}

async fn handle_device(account: &AccountHandle, command: DeviceCommands) -> Result<()> {
    match command {
        DeviceCommands::List => {
            println!("{}", "Authorized Devices".bright_blue().bold());
            println!("===================");

            let devices = account.list_authorized_devices().await?;
            if devices.is_empty() {
                println!("No authorized devices found.");
                return Ok(());
            }

            for device in devices {
                println!(
                    "\n{}",
                    format!("Device: {}", device.device_id).yellow().bold()
                );
                println!("  Name: {}", device.device_name.bright_blue());
                println!(
                    "  Authorized: {}",
                    if device.is_authorized {
                        "Yes".green()
                    } else {
                        "No".red()
                    }
                );
                println!(
                    "  Last Seen: {}",
                    format!("{}", device.last_seen.format("%Y-%m-%d %H:%M:%S UTC")).bright_blue()
                );
                println!(
                    "  Online: {}",
                    if device.is_online {
                        "Yes".green()
                    } else {
                        "No".red()
                    }
                );
            }
        }
        DeviceCommands::Info { device_id } => {
            println!(
                "{}",
                format!("Device Info: {}", device_id).bright_blue().bold()
            );
            println!("========================");

            let devices = account.list_authorized_devices().await?;
            if let Some(device) = devices.iter().find(|d| d.device_id == device_id) {
                println!("  Device ID: {}", device.device_id.bright_blue());
                println!("  Name: {}", device.device_name.bright_blue());
                println!(
                    "  Authorized: {}",
                    if device.is_authorized {
                        "Yes".green()
                    } else {
                        "No".red()
                    }
                );
                println!(
                    "  Last Seen: {}",
                    format!("{}", device.last_seen.format("%Y-%m-%d %H:%M:%S UTC")).bright_blue()
                );
                println!(
                    "  Online: {}",
                    if device.is_online {
                        "Yes".green()
                    } else {
                        "No".red()
                    }
                );

                if let Some(cert) = &device.device_cert {
                    println!(
                        "  Certificate Issued: {}",
                        format!("{}", cert.issued_at.format("%Y-%m-%d %H:%M:%S UTC")).bright_blue()
                    );
                    if let Some(expires_at) = cert.expires_at {
                        println!(
                            "  Certificate Expires: {}",
                            format!("{}", expires_at.format("%Y-%m-%d %H:%M:%S UTC")).bright_blue()
                        );
                    }
                }
            } else {
                println!("Device not found: {}", device_id.red());
            }
        }
        DeviceCommands::Link => {
            println!(
                "{}",
                "Device linking requires daemon to be running".yellow()
            );
            println!("Please start the daemon with: cargo run --bin saved-daemon");
        }
        DeviceCommands::Accept { qr_payload: _ } => {
            println!(
                "{}",
                "Device linking requires daemon to be running".yellow()
            );
            println!("Please start the daemon with: cargo run --bin saved-daemon");
        }
        DeviceCommands::Revoke { device_id: _ } => {
            println!(
                "{}",
                "Device revocation requires daemon to be running".yellow()
            );
            println!("Please start the daemon with: cargo run --bin saved-daemon");
        }
    }
    Ok(())
}

async fn handle_peer(account: &AccountHandle, command: PeerCommands) -> Result<()> {
    match command {
        PeerCommands::List => {
            println!("{}", "Peers".bright_blue().bold());
            println!("======");

            // Connected peers
            let connected_peers = account.connected_peers().await;
            let connected_count = connected_peers.len();
            if !connected_peers.is_empty() {
                println!("\n{}", "Connected Peers".green().bold());
                for (device_id, device_info) in connected_peers {
                    let connection_state = account.peer_connection_state(&device_id).await;
                    let health = account.peer_health(&device_id).await;

                    println!(
                        "  {} ({})",
                        device_info.device_name.bright_blue(),
                        device_id.bright_blue()
                    );
                    println!("    Status: {}", "Connected".green());
                    println!(
                        "    Connection: {}",
                        connection_state
                            .unwrap_or_else(|| "Unknown".to_string())
                            .bright_blue()
                    );
                    println!(
                        "    Health: {}",
                        health
                            .unwrap_or_else(|| "Unknown".to_string())
                            .bright_blue()
                    );
                    println!(
                        "    Last Seen: {}",
                        format!("{}", device_info.last_seen.format("%Y-%m-%d %H:%M:%S UTC"))
                            .bright_blue()
                    );
                }
            }

            // Discovered peers
            let discovered_peers = account.discovered_peers().await;
            let discovered_count = discovered_peers.len();
            if !discovered_peers.is_empty() {
                println!("\n{}", "Discovered Peers".yellow().bold());
                for (device_id, discovery_info) in discovered_peers {
                    println!(
                        "  {} ({})",
                        format!("Device {}", &device_id[..8]).bright_blue(),
                        device_id.bright_blue()
                    );
                    println!("    Status: {}", "Discovered".yellow());
                    println!(
                        "    Discovery Method: {}",
                        format!("{:?}", discovery_info.service_type).bright_blue()
                    );
                    println!(
                        "    Addresses: {}",
                        discovery_info.addresses.join(", ").bright_blue()
                    );
                }
            }

            if connected_count == 0 && discovered_count == 0 {
                println!("No peers found. Use 'savedctl peer scan' to discover peers.");
            }
        }
        PeerCommands::Connect {
            device_id,
            addresses,
        } => {
            println!("Connecting to peer: {}", device_id.bright_blue());
            if let Some(addresses_str) = addresses {
                println!("Using addresses: {}", addresses_str.bright_blue());
            }
            // TODO: Implement peer connection
            println!("Peer connection not yet implemented.");
        }
        PeerCommands::Disconnect { device_id } => {
            println!("Disconnecting from peer: {}", device_id.bright_blue());
            // TODO: Implement peer disconnection
            println!("Peer disconnection not yet implemented.");
        }
        PeerCommands::Scan => {
            println!("Scanning for peers...");
            // TODO: Implement peer scanning
            println!("Peer scanning not yet implemented.");
        }
    }
    Ok(())
}

async fn handle_message(account: &mut AccountHandle, command: MessageCommands) -> Result<()> {
    match command {
        MessageCommands::List => {
            println!("{}", "Messages".bright_blue().bold());
            println!("========");

            let messages = account.list_messages().await?;
            if messages.is_empty() {
                println!("No messages found.");
                return Ok(());
            }

            for message in messages {
                println!("\n{}", format!("Message: {:?}", message.id).yellow().bold());
                println!("  Content: {}", message.content.bright_blue());
                println!(
                    "  Created: {}",
                    format!("{}", message.created_at.format("%Y-%m-%d %H:%M:%S UTC")).bright_blue()
                );
            }
        }
        MessageCommands::Send {
            content,
            attachments,
        } => {
            println!("Sending message: {}", content.bright_blue());
            let attachment_paths: Vec<std::path::PathBuf> = attachments
                .map(|a| {
                    a.split(',')
                        .map(|s| s.trim().to_string())
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.into())
                .collect();
            let message_id = account.create_message(content, attachment_paths).await?;
            println!(
                "Message sent with ID: {}",
                format!("{:?}", message_id).bright_green()
            );
        }
        MessageCommands::Edit {
            message_id,
            new_content: _,
        } => {
            println!("Editing message: {}", message_id.bright_blue());
            // TODO: Implement message editing
            println!("Message editing not yet implemented.");
        }
        MessageCommands::Delete { message_id } => {
            println!("Deleting message: {}", message_id.bright_blue());
            // TODO: Implement message deletion
            println!("Message deletion not yet implemented.");
        }
        MessageCommands::Purge { message_id } => {
            println!("Purging message: {}", message_id.bright_blue());
            // TODO: Implement message purging
            println!("Message purging not yet implemented.");
        }
    }
    Ok(())
}

async fn handle_network(account: &AccountHandle, command: NetworkCommands) -> Result<()> {
    match command {
        NetworkCommands::Status => {
            println!("{}", "Network Status".bright_blue().bold());
            println!("===============");

            let connected_count = account.connected_peers_count().await;
            let discovered_peers = account.discovered_peers().await;
            let discovered_count = discovered_peers.len();

            println!(
                "  Connected Peers: {}",
                connected_count.to_string().bright_blue()
            );
            println!(
                "  Discovered Peers: {}",
                discovered_count.to_string().bright_blue()
            );
            println!(
                "  Network Status: {}",
                if connected_count > 0 || discovered_count > 0 {
                    "Active".green()
                } else {
                    "Inactive".red()
                }
            );
        }
        NetworkCommands::Addresses => {
            println!("{}", "Network Addresses".bright_blue().bold());
            println!("===================");
            println!("Network address display not yet implemented.");
        }
        NetworkCommands::Start => {
            println!("Starting network...");
            // TODO: Implement network start
            println!("Network start not yet implemented.");
        }
        NetworkCommands::Stop => {
            println!("Stopping network...");
            // TODO: Implement network stop
            println!("Network stop not yet implemented.");
        }
        NetworkCommands::Scan => {
            println!("Scanning network...");
            // TODO: Implement network scanning
            println!("Network scanning not yet implemented.");
        }
    }
    Ok(())
}

// Daemon communication handlers
async fn handle_status_via_daemon(client: &DaemonClient) -> Result<()> {
    let response = client.send_request(DaemonRequest::Status).await?;

    match response {
        DaemonResponse::Status {
            device_id,
            device_name,
            authorized,
            last_seen,
            connected_peers,
            discovered_peers,
            total_messages,
            total_devices,
        } => {
            println!("{}", "SAVED Status".bright_blue().bold());
            println!("=============");

            // Device info
            println!("\n{}", "Device Information".yellow().bold());
            println!("  Device ID: {}", device_id.bright_blue());
            println!("  Device Name: {}", device_name.bright_blue());
            println!(
                "  Authorized: {}",
                if authorized {
                    "Yes".green()
                } else {
                    "No".red()
                }
            );
            println!("  Last Seen: {}", last_seen.bright_blue());

            // Network status
            println!("\n{}", "Network Status".yellow().bold());
            println!(
                "  Connected Peers: {}",
                connected_peers.to_string().bright_blue()
            );
            println!(
                "  Discovered Peers: {}",
                discovered_peers.to_string().bright_blue()
            );
            println!(
                "  Network Status: {}",
                if connected_peers > 0 || discovered_peers > 0 {
                    "Active".green()
                } else {
                    "Inactive".red()
                }
            );

            // Messages
            println!("\n{}", "Messages".yellow().bold());
            println!(
                "  Total Messages: {}",
                total_messages.to_string().bright_blue()
            );

            // Devices
            println!("\n{}", "Authorized Devices".yellow().bold());
            println!(
                "  Total Devices: {}",
                total_devices.to_string().bright_blue()
            );
        }
        DaemonResponse::Error(msg) => {
            println!("{}", format!("Error: {}", msg).red());
        }
        _ => {
            println!("{}", "Unexpected response type".red());
        }
    }

    Ok(())
}

async fn handle_device_via_daemon(client: &DaemonClient, command: DeviceCommands) -> Result<()> {
    match command {
        DeviceCommands::List => {
            let response = client.send_request(DaemonRequest::DeviceList).await?;
            match response {
                DaemonResponse::DeviceList(devices) => {
                    println!("{}", "Authorized Devices".bright_blue().bold());
                    println!("===================");

                    if devices.is_empty() {
                        println!("No authorized devices found.");
                        return Ok(());
                    }

                    for device in devices {
                        println!(
                            "\n{}",
                            format!("Device: {}", device.device_id).yellow().bold()
                        );
                        println!("  Name: {}", device.device_name.bright_blue());
                        println!(
                            "  Authorized: {}",
                            if device.is_authorized {
                                "Yes".green()
                            } else {
                                "No".red()
                            }
                        );
                        println!("  Last Seen: {}", device.last_seen.bright_blue());
                        println!(
                            "  Online: {}",
                            if device.is_online {
                                "Yes".green()
                            } else {
                                "No".red()
                            }
                        );
                    }
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        DeviceCommands::Info { device_id } => {
            let response = client
                .send_request(DaemonRequest::DeviceInfo {
                    device_id: device_id.clone(),
                })
                .await?;
            match response {
                DaemonResponse::DeviceInfo(Some(device)) => {
                    println!(
                        "{}",
                        format!("Device Info: {}", device_id).bright_blue().bold()
                    );
                    println!("========================");
                    println!("  Device ID: {}", device.device_id.bright_blue());
                    println!("  Name: {}", device.device_name.bright_blue());
                    println!(
                        "  Authorized: {}",
                        if device.is_authorized {
                            "Yes".green()
                        } else {
                            "No".red()
                        }
                    );
                    println!("  Last Seen: {}", device.last_seen.bright_blue());
                    println!(
                        "  Online: {}",
                        if device.is_online {
                            "Yes".green()
                        } else {
                            "No".red()
                        }
                    );
                }
                DaemonResponse::DeviceInfo(None) => {
                    println!("Device not found: {}", device_id.red());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        DeviceCommands::Link => {
            let response = client.send_request(DaemonRequest::DeviceLink).await?;
            match response {
                DaemonResponse::DeviceLink { qr_payload } => {
                    println!("{}", "Device Linking QR Code".bright_blue().bold());
                    println!("========================");
                    println!("QR Payload: {}", qr_payload.bright_blue());
                    println!("\nShare this QR code with the device you want to link.");
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        DeviceCommands::Accept { qr_payload } => {
            let response = client
                .send_request(DaemonRequest::DeviceAccept {
                    qr_payload: qr_payload.clone(),
                })
                .await?;
            match response {
                DaemonResponse::DeviceAccepted {
                    device_id,
                    device_name,
                } => {
                    println!("Device link accepted successfully!");
                    println!("  Device ID: {}", device_id.bright_blue());
                    println!("  Device Name: {}", device_name.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        DeviceCommands::Revoke { device_id } => {
            let response = client
                .send_request(DaemonRequest::DeviceRevoke {
                    device_id: device_id.clone(),
                })
                .await?;
            match response {
                DaemonResponse::Success => {
                    println!("Device {} revoked successfully.", device_id.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
    }
    Ok(())
}

async fn handle_peer_via_daemon(client: &DaemonClient, command: PeerCommands) -> Result<()> {
    match command {
        PeerCommands::List => {
            let response = client.send_request(DaemonRequest::PeerList).await?;
            match response {
                DaemonResponse::PeerList {
                    connected,
                    discovered,
                } => {
                    println!("{}", "Peers".bright_blue().bold());
                    println!("======");

                    if !connected.is_empty() {
                        println!("\n{}", "Connected Peers".green().bold());
                        for peer in &connected {
                            println!(
                                "  {} ({})",
                                peer.device_name.bright_blue(),
                                peer.device_id.bright_blue()
                            );
                            println!("    Status: {}", "Connected".green());
                            println!(
                                "    Connection: {}",
                                peer.connection_state
                                    .as_ref()
                                    .unwrap_or(&"Unknown".to_string())
                                    .bright_blue()
                            );
                            println!(
                                "    Health: {}",
                                peer.health
                                    .as_ref()
                                    .unwrap_or(&"Unknown".to_string())
                                    .bright_blue()
                            );
                            println!("    Last Seen: {}", peer.last_seen.bright_blue());
                        }
                    }

                    if !discovered.is_empty() {
                        println!("\n{}", "Discovered Peers".yellow().bold());
                        for peer in &discovered {
                            println!(
                                "  {} ({})",
                                format!("Device {}", &peer.device_id[..8]).bright_blue(),
                                peer.device_id.bright_blue()
                            );
                            println!("    Status: {}", "Discovered".yellow());
                            println!("    Discovery Method: {}", peer.service_type.bright_blue());
                            println!("    Addresses: {}", peer.addresses.join(", ").bright_blue());
                        }
                    }

                    if connected.is_empty() && discovered.is_empty() {
                        println!("No peers found. Use 'savedctl peer scan' to discover peers.");
                    }
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        PeerCommands::Connect {
            device_id,
            addresses,
        } => {
            let addresses_vec = addresses
                .map(|a| a.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();
            let response = client
                .send_request(DaemonRequest::PeerConnect {
                    device_id: device_id.clone(),
                    addresses: addresses_vec,
                })
                .await?;
            match response {
                DaemonResponse::Success => {
                    println!("Connecting to peer: {}", device_id.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        PeerCommands::Disconnect { device_id } => {
            let response = client
                .send_request(DaemonRequest::PeerDisconnect {
                    device_id: device_id.clone(),
                })
                .await?;
            match response {
                DaemonResponse::Success => {
                    println!("Disconnecting from peer: {}", device_id.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        PeerCommands::Scan => {
            let response = client.send_request(DaemonRequest::PeerScan).await?;
            match response {
                DaemonResponse::Success => {
                    println!("Scanning for peers...");
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
    }
    Ok(())
}

async fn handle_message_via_daemon(client: &DaemonClient, command: MessageCommands) -> Result<()> {
    match command {
        MessageCommands::List => {
            let response = client.send_request(DaemonRequest::MessageList).await?;
            match response {
                DaemonResponse::MessageList(messages) => {
                    println!("{}", "Messages".bright_blue().bold());
                    println!("========");

                    if messages.is_empty() {
                        println!("No messages found.");
                        return Ok(());
                    }

                    for message in messages {
                        println!("\n{}", format!("Message: {}", message.id).yellow().bold());
                        println!("  Content: {}", message.content.bright_blue());
                        println!("  Created: {}", message.created_at.bright_blue());
                    }
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        MessageCommands::Send {
            content,
            attachments,
        } => {
            let attachments_vec = attachments
                .map(|a| a.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();
            let response = client
                .send_request(DaemonRequest::MessageSend {
                    content: content.clone(),
                    attachments: attachments_vec,
                })
                .await?;
            match response {
                DaemonResponse::MessageSent { message_id } => {
                    println!("Sending message: {}", content.bright_blue());
                    println!("Message sent with ID: {}", message_id.bright_green());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        MessageCommands::Edit {
            message_id,
            new_content,
        } => {
            let response = client
                .send_request(DaemonRequest::MessageEdit {
                    message_id: message_id.clone(),
                    new_content: new_content.clone(),
                })
                .await?;
            match response {
                DaemonResponse::MessageEdited { message_id } => {
                    println!("Message {} edited successfully.", message_id.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        MessageCommands::Delete { message_id } => {
            let response = client
                .send_request(DaemonRequest::MessageDelete {
                    message_id: message_id.clone(),
                })
                .await?;
            match response {
                DaemonResponse::Success => {
                    println!("Deleting message: {}", message_id.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        MessageCommands::Purge { message_id } => {
            let response = client
                .send_request(DaemonRequest::MessagePurge {
                    message_id: message_id.clone(),
                })
                .await?;
            match response {
                DaemonResponse::Success => {
                    println!("Purging message: {}", message_id.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
    }
    Ok(())
}

async fn handle_network_via_daemon(client: &DaemonClient, command: NetworkCommands) -> Result<()> {
    match command {
        NetworkCommands::Status => {
            let response = client.send_request(DaemonRequest::NetworkStatus).await?;
            match response {
                DaemonResponse::NetworkStatus {
                    connected_peers,
                    discovered_peers,
                    active,
                } => {
                    println!("{}", "Network Status".bright_blue().bold());
                    println!("===============");
                    println!(
                        "  Connected Peers: {}",
                        connected_peers.to_string().bright_blue()
                    );
                    println!(
                        "  Discovered Peers: {}",
                        discovered_peers.to_string().bright_blue()
                    );
                    println!(
                        "  Network Status: {}",
                        if active {
                            "Active".green()
                        } else {
                            "Inactive".red()
                        }
                    );
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        NetworkCommands::Addresses => {
            let response = client.send_request(DaemonRequest::NetworkAddresses).await?;
            match response {
                DaemonResponse::NetworkAddresses(addresses) => {
                    println!("{}", "Network Addresses".bright_blue().bold());
                    println!("===================");
                    for addr in addresses {
                        println!("  {}", addr.bright_blue());
                    }
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        NetworkCommands::Start => {
            let response = client.send_request(DaemonRequest::NetworkStart).await?;
            match response {
                DaemonResponse::Success => {
                    println!("Starting network...");
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        NetworkCommands::Stop => {
            let response = client.send_request(DaemonRequest::NetworkStop).await?;
            match response {
                DaemonResponse::Success => {
                    println!("Stopping network...");
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        NetworkCommands::Scan => {
            let response = client.send_request(DaemonRequest::NetworkScan).await?;
            match response {
                DaemonResponse::NetworkScanned { discovered_count } => {
                    println!(
                        "Network scan completed. Found {} peers.",
                        discovered_count.to_string().bright_blue()
                    );
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
    }
    Ok(())
}

async fn handle_attachment_via_daemon(
    client: &DaemonClient,
    command: AttachmentCommands,
) -> Result<()> {
    match command {
        AttachmentCommands::List => {
            let response = client.send_request(DaemonRequest::AttachmentList).await?;
            match response {
                DaemonResponse::AttachmentList(attachments) => {
                    println!("{}", "Attachments".bright_blue().bold());
                    println!("============");
                    if attachments.is_empty() {
                        println!("No attachments found.");
                    } else {
                        for attachment in attachments {
                            println!("Attachment ID: {}", attachment.id.to_string().bright_blue());
                            println!("  Message ID: {}", attachment.message_id.bright_blue());
                            println!("  Filename: {}", attachment.filename.bright_blue());
                            println!(
                                "  Size: {} bytes",
                                attachment.size.to_string().bright_blue()
                            );
                            if let Some(mime_type) = attachment.mime_type {
                                println!("  MIME Type: {}", mime_type.bright_blue());
                            }
                            println!("  Status: {}", attachment.status.bright_blue());
                            println!("  Created: {}", attachment.created_at.bright_blue());
                            if let Some(last_accessed) = attachment.last_accessed {
                                println!("  Last Accessed: {}", last_accessed.bright_blue());
                            }
                            println!();
                        }
                    }
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        AttachmentCommands::Download {
            attachment_id,
            output_path,
        } => {
            let response = client
                .send_request(DaemonRequest::AttachmentDownload {
                    attachment_id,
                    output_path: output_path.clone(),
                })
                .await?;
            match response {
                DaemonResponse::AttachmentDownloaded {
                    attachment_id,
                    output_path,
                } => {
                    println!(
                        "Downloaded attachment {} to {}",
                        attachment_id.to_string().bright_blue(),
                        output_path.bright_blue()
                    );
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        AttachmentCommands::Delete { attachment_id } => {
            let response = client
                .send_request(DaemonRequest::AttachmentDelete { attachment_id })
                .await?;
            match response {
                DaemonResponse::Success => {
                    println!(
                        "Deleted attachment {}",
                        attachment_id.to_string().bright_blue()
                    );
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        AttachmentCommands::Purge { attachment_id } => {
            let response = client
                .send_request(DaemonRequest::AttachmentPurge { attachment_id })
                .await?;
            match response {
                DaemonResponse::Success => {
                    println!(
                        "Purged attachment {}",
                        attachment_id.to_string().bright_blue()
                    );
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
    }
    Ok(())
}

async fn handle_account_via_daemon(client: &DaemonClient, command: AccountCommands) -> Result<()> {
    match command {
        AccountCommands::Export { output_path } => {
            let response = client
                .send_request(DaemonRequest::AccountExport {
                    output_path: output_path.clone(),
                })
                .await?;
            match response {
                DaemonResponse::AccountExported { output_path } => {
                    println!("Account exported to {}", output_path.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        AccountCommands::Import { input_path } => {
            let response = client
                .send_request(DaemonRequest::AccountImport {
                    input_path: input_path.clone(),
                })
                .await?;
            match response {
                DaemonResponse::AccountImported { messages_imported } => {
                    println!(
                        "Account imported from {}. {} messages imported.",
                        input_path.bright_blue(),
                        messages_imported.to_string().bright_blue()
                    );
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
    }
    Ok(())
}

// Fallback handlers for direct database access when daemon is not running
async fn handle_attachment(_account: &AccountHandle, _command: AttachmentCommands) -> Result<()> {
    println!(
        "{}",
        "Attachment management requires daemon to be running".yellow()
    );
    println!("Please start the daemon with: cargo run --bin saved-daemon");
    Ok(())
}

async fn handle_account(_account: &AccountHandle, _command: AccountCommands) -> Result<()> {
    println!(
        "{}",
        "Account management requires daemon to be running".yellow()
    );
    println!("Please start the daemon with: cargo run --bin saved-daemon");
    Ok(())
}

async fn handle_chunk_via_daemon(client: &DaemonClient, command: ChunkCommands) -> Result<()> {
    match command {
        ChunkCommands::Init => {
            let response = client
                .send_request(DaemonRequest::InitializeChunkSync)
                .await?;
            match response {
                DaemonResponse::ChunkSyncInitialized => {
                    println!("{}", "Chunk synchronization initialized".green());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        ChunkCommands::Store { file_path } => {
            let data = std::fs::read(&file_path)?;
            let response = client
                .send_request(DaemonRequest::StoreChunk { data })
                .await?;
            match response {
                DaemonResponse::ChunkStored { chunk_id } => {
                    println!("Chunk stored with ID: {}", chunk_id.bright_blue());
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        ChunkCommands::Get {
            chunk_id,
            output_path,
        } => {
            let response = client
                .send_request(DaemonRequest::GetChunk {
                    chunk_id: chunk_id.clone(),
                })
                .await?;
            match response {
                DaemonResponse::ChunkData { chunk_id: _, data } => {
                    if let Some(data) = data {
                        std::fs::write(&output_path, data)?;
                        println!(
                            "Chunk {} saved to {}",
                            chunk_id.bright_blue(),
                            output_path.bright_blue()
                        );
                    } else {
                        println!("{}", "Chunk not found".red());
                    }
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        ChunkCommands::Check { chunk_ids } => {
            let response = client
                .send_request(DaemonRequest::CheckChunkAvailability {
                    chunk_ids: chunk_ids.clone(),
                })
                .await?;
            match response {
                DaemonResponse::ChunkAvailability { availability } => {
                    println!("Chunk availability:");
                    for (chunk_id, is_available) in availability {
                        let status = if is_available {
                            "Available".green()
                        } else {
                            "Not available".red()
                        };
                        println!("  {}: {}", chunk_id.bright_blue(), status);
                    }
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
        ChunkCommands::Fetch { chunk_ids } => {
            let response = client
                .send_request(DaemonRequest::FetchMissingChunks {
                    chunk_ids: chunk_ids.clone(),
                })
                .await?;
            match response {
                DaemonResponse::ChunksFetched { fetched_count } => {
                    println!(
                        "Fetched {} chunks from peers",
                        fetched_count.to_string().bright_blue()
                    );
                }
                DaemonResponse::Error(msg) => {
                    println!("{}", format!("Error: {}", msg).red());
                }
                _ => {
                    println!("{}", "Unexpected response type".red());
                }
            }
        }
    }
    Ok(())
}

async fn handle_chunk(account: &mut AccountHandle, command: ChunkCommands) -> Result<()> {
    match command {
        ChunkCommands::Init => {
            account.initialize_chunk_sync().await?;
            println!("{}", "Chunk synchronization initialized".green());
        }
        ChunkCommands::Store { file_path } => {
            let data = std::fs::read(&file_path)?;
            let chunk_id = account.store_chunk(&data).await?;
            println!(
                "Chunk stored with ID: {}",
                format!("{:02x?}", chunk_id)
                    .replace(" ", "")
                    .replace("[", "")
                    .replace("]", "")
                    .bright_blue()
            );
        }
        ChunkCommands::Get {
            chunk_id,
            output_path,
        } => {
            // Parse chunk ID from hex string
            let chunk_id_bytes = if chunk_id.len() == 64 {
                match hex::decode(&chunk_id) {
                    Ok(bytes) if bytes.len() == 32 => {
                        let mut chunk_id_array = [0u8; 32];
                        chunk_id_array.copy_from_slice(&bytes);
                        chunk_id_array
                    }
                    _ => return Err(anyhow::anyhow!("Invalid chunk ID format")),
                }
            } else {
                return Err(anyhow::anyhow!("Chunk ID must be 64 hex characters"));
            };

            if let Some(data) = account.get_chunk(&chunk_id_bytes).await? {
                std::fs::write(&output_path, data)?;
                println!(
                    "Chunk {} saved to {}",
                    chunk_id.bright_blue(),
                    output_path.bright_blue()
                );
            } else {
                println!("{}", "Chunk not found".red());
            }
        }
        ChunkCommands::Check { chunk_ids } => {
            // Parse chunk IDs
            let mut chunk_id_bytes = Vec::new();
            for chunk_id in &chunk_ids {
                if chunk_id.len() == 64 {
                    match hex::decode(chunk_id) {
                        Ok(bytes) if bytes.len() == 32 => {
                            let mut chunk_id_array = [0u8; 32];
                            chunk_id_array.copy_from_slice(&bytes);
                            chunk_id_bytes.push(chunk_id_array);
                        }
                        _ => return Err(anyhow::anyhow!("Invalid chunk ID format: {}", chunk_id)),
                    }
                } else {
                    return Err(anyhow::anyhow!(
                        "Chunk ID must be 64 hex characters: {}",
                        chunk_id
                    ));
                }
            }

            let availability = account.check_chunk_availability(&chunk_id_bytes).await?;
            println!("Chunk availability:");
            for (i, chunk_id) in chunk_ids.iter().enumerate() {
                if i < chunk_id_bytes.len() {
                    let is_available = availability
                        .get(&chunk_id_bytes[i])
                        .copied()
                        .unwrap_or(false);
                    let status = if is_available {
                        "Available".green()
                    } else {
                        "Not available".red()
                    };
                    println!("  {}: {}", chunk_id.bright_blue(), status);
                }
            }
        }
        ChunkCommands::Fetch { chunk_ids } => {
            // Parse chunk IDs
            let mut chunk_id_bytes = Vec::new();
            for chunk_id in &chunk_ids {
                if chunk_id.len() == 64 {
                    match hex::decode(chunk_id) {
                        Ok(bytes) if bytes.len() == 32 => {
                            let mut chunk_id_array = [0u8; 32];
                            chunk_id_array.copy_from_slice(&bytes);
                            chunk_id_bytes.push(chunk_id_array);
                        }
                        _ => return Err(anyhow::anyhow!("Invalid chunk ID format: {}", chunk_id)),
                    }
                } else {
                    return Err(anyhow::anyhow!(
                        "Chunk ID must be 64 hex characters: {}",
                        chunk_id
                    ));
                }
            }

            account.fetch_missing_chunks(&chunk_id_bytes).await?;
            println!(
                "Fetched {} chunks from peers",
                chunk_ids.len().to_string().bright_blue()
            );
        }
    }
    Ok(())
}
