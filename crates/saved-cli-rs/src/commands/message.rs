//! Message management commands

use anyhow::Result;
use colored::*;
use comfy_table::{Cell, Table};
use saved_core_rs::{create_or_open_account, Config, MessageId};
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
            println!("Attachments: {:?}", attachments);
        }
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
    limit: Option<usize>,
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
        storage_backend: saved_core_rs::storage::StorageBackend::Sqlite,
    };

    // Open account
    let account = create_or_open_account(config).await?;

    if ids_only {
        // For now, just show a placeholder since we don't have message storage implemented yet
        println!("{}", "Message IDs:".bright_blue().bold());
        println!("(Message storage not yet implemented in core library)");
    } else {
        // Create a table for better formatting
        let mut table = Table::new();
        table.set_header(vec![
            Cell::new("Message ID").add_attribute(comfy_table::Attribute::Bold),
            Cell::new("Content").add_attribute(comfy_table::Attribute::Bold),
            Cell::new("Created").add_attribute(comfy_table::Attribute::Bold),
        ]);

        // For now, show placeholder data
        table.add_row(vec![
            Cell::new("(not implemented)"),
            Cell::new("Message storage not yet implemented in core library"),
            Cell::new("N/A"),
        ]);

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
        storage_backend: saved_core_rs::storage::StorageBackend::Sqlite,
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
        storage_backend: saved_core_rs::storage::StorageBackend::Sqlite,
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
    let _msg_id = parse_message_id(message_id)?;

    // For now, just show a placeholder since we don't have message storage implemented yet
    println!("{}", "Message Details:".bright_blue().bold());
    println!("Message ID: {}", message_id.bright_blue());
    println!(
        "Content: {}",
        "(Message storage not yet implemented in core library)".yellow()
    );

    Ok(())
}

/// Parse a hex-encoded message ID
fn parse_message_id(hex_str: &str) -> Result<MessageId> {
    let bytes = hex::decode(hex_str).map_err(|_| anyhow::anyhow!("Invalid message ID format"))?;

    if bytes.len() != 32 {
        return Err(anyhow::anyhow!(
            "Message ID must be 32 bytes (64 hex characters)"
        ));
    }

    let mut msg_id_bytes = [0u8; 32];
    msg_id_bytes.copy_from_slice(&bytes);

    Ok(MessageId::from_bytes(msg_id_bytes))
}
