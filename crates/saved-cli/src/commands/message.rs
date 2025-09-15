//! Message management commands

use crate::utils::formatting::{format_file_size, format_message_id, format_short_message_id};
use crate::utils::validation::{
    validate_attachment, validate_message_content, validate_message_id,
};
use anyhow::Result;
use colored::*;
use comfy_table::{Cell, Table};
use saved_core::{create_or_open_account, Config, MessageId};
use std::path::PathBuf;

/// Create a new message
pub async fn create_command(
    account_path: &PathBuf,
    content: &str,
    attachments: Vec<PathBuf>,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Creating new message...");
        println!("Content: {}", content);
        if !attachments.is_empty() {
            println!("Attachments:");
            for attachment in &attachments {
                if let Ok(metadata) = std::fs::metadata(attachment) {
                    println!(
                        "  • {} ({})",
                        attachment.display(),
                        format_file_size(metadata.len())
                    );
                } else {
                    println!("  • {} (size unknown)", attachment.display());
                }
            }
        }
    }

    // Validate message content
    validate_message_content(content)?;

    // Validate attachments
    for attachment in &attachments {
        validate_attachment(attachment)?;
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

    // Create message
    let message_id = account
        .create_message(content.to_string(), attachments)
        .await?;

    println!("{}", "✓ Message created successfully!".green().bold());
    println!(
        "Message ID: {}",
        hex::encode(message_id.as_bytes()).bright_blue()
    );
    println!("Content: {}", content.bright_blue());

    Ok(())
}

/// List all messages
pub async fn list_command(
    account_path: &PathBuf,
    ids_only: bool,
    _limit: Option<usize>,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Listing messages...");
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

    // Get messages from the account
    let messages = account
        .list_messages()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list messages: {}", e))?;

    if messages.is_empty() {
        println!("{}", "No messages found.".yellow());
        return Ok(());
    }

    if ids_only {
        println!("{}", "Message IDs:".bright_blue().bold());
        for message in messages {
            println!("{}", format_short_message_id(&message.id.0).bright_blue());
        }
    } else {
        // Create a table for better formatting
        let mut table = Table::new();
        table.set_header(vec![
            Cell::new("Message ID").add_attribute(comfy_table::Attribute::Bold),
            Cell::new("Content").add_attribute(comfy_table::Attribute::Bold),
            Cell::new("Created").add_attribute(comfy_table::Attribute::Bold),
            Cell::new("Status").add_attribute(comfy_table::Attribute::Bold),
        ]);

        for message in messages {
            let content_preview = if message.content.len() > 50 {
                format!("{}...", &message.content[..50])
            } else {
                message.content.clone()
            };

            let status = if message.is_purged {
                "Purged".red()
            } else if message.is_deleted {
                "Deleted".yellow()
            } else {
                "Active".green()
            };

            table.add_row(vec![
                Cell::new(format_short_message_id(&message.id.0)),
                Cell::new(content_preview),
                Cell::new(message.created_at.format("%Y-%m-%d %H:%M:%S").to_string()),
                Cell::new(status),
            ]);
        }

        println!("{}", "Messages:".bright_blue().bold());
        println!("{}", table);
    }

    Ok(())
}

/// Edit an existing message
pub async fn edit_command(
    account_path: &PathBuf,
    message_id: &str,
    content: &str,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Editing message...");
        println!("Message ID: {}", message_id);
        println!("New content: {}", content);
    }

    // Validate message content
    validate_message_content(content)?;

    // Parse message ID
    let msg_id = parse_message_id(message_id)?;

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

    // Edit message
    account.edit_message(msg_id, content.to_string()).await?;

    println!("{}", "✓ Message edited successfully!".green().bold());
    println!("Message ID: {}", message_id.bright_blue());
    println!("New content: {}", content.bright_blue());

    Ok(())
}

/// Delete a message
pub async fn delete_command(
    account_path: &PathBuf,
    message_id: &str,
    permanent: bool,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Deleting message...");
        println!("Message ID: {}", message_id);
        println!("Permanent: {}", permanent);
    }

    // Parse message ID
    let msg_id = parse_message_id(message_id)?;

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

    if permanent {
        account.purge_message(msg_id).await?;
        println!("{}", "✓ Message permanently deleted!".green().bold());
    } else {
        account.delete_message(msg_id).await?;
        println!("{}", "✓ Message deleted!".green().bold());
    }

    println!("Message ID: {}", message_id.bright_blue());

    Ok(())
}

/// Show message content
pub async fn show_command(account_path: &PathBuf, message_id: &str, verbose: bool) -> Result<()> {
    if verbose {
        println!("Showing message...");
        println!("Message ID: {}", message_id);
    }

    // Parse message ID
    let msg_id = parse_message_id(message_id)?;

    // Create account configuration
    let config = saved_core::types::Config {
        storage_path: account_path.clone(),
        network_port: 0,
        enable_mdns: false,
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

    // Get all messages and find the one with matching ID
    let messages = account
        .list_messages()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list messages: {}", e))?;

    let message = messages
        .iter()
        .find(|m| m.id == msg_id)
        .ok_or_else(|| anyhow::anyhow!("Message with ID {} not found", message_id))?;

    // Display message details
    println!("{}", "Message Details:".bright_blue().bold());
    println!(
        "Message ID: {}",
        format_short_message_id(&message.id.0).bright_blue()
    );
    println!("Content: {}", message.content);
    println!(
        "Created: {}",
        message
            .created_at
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string()
            .bright_blue()
    );

    let status = if message.is_purged {
        "Purged".red()
    } else if message.is_deleted {
        "Deleted".yellow()
    } else {
        "Active".green()
    };
    println!("Status: {}", status);

    if verbose {
        println!("Content Length: {} characters", message.content.len());
        println!("Message ID (full): {}", format_message_id(&message.id.0));
    }

    Ok(())
}

/// Parse a hex-encoded message ID
fn parse_message_id(hex_str: &str) -> Result<MessageId> {
    validate_message_id(hex_str)?;
    let bytes = hex::decode(hex_str).map_err(|_| anyhow::anyhow!("Invalid message ID format"))?;
    let mut msg_id_bytes = [0u8; 32];
    msg_id_bytes.copy_from_slice(&bytes);

    Ok(MessageId::from_bytes(msg_id_bytes))
}
