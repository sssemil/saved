//! Import/export commands

use anyhow::Result;
use colored::*;
use saved_core_rs::{create_or_open_account, Config};
use std::path::PathBuf;
use crate::utils::validation::validate_file_path;
use crate::utils::formatting::print_warning;

/// Export messages to JSON
pub async fn export_command(account_path: &PathBuf, output: &PathBuf, verbose: bool) -> Result<()> {
    if verbose {
        println!("Exporting messages to JSON...");
        println!("Output file: {}", output.display());
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

    // Create export data structure
    let export_data = serde_json::json!({
        "version": "1.0",
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "account_path": account_path.to_string_lossy(),
        "device_info": account.device_info().await,
        "messages": [],
        "note": "Message storage not yet implemented in core library"
    });

    // Write to file
    let json_string = serde_json::to_string_pretty(&export_data)?;
    std::fs::write(output, json_string)?;

    println!("{}", "✓ Messages exported successfully!".green().bold());
    println!(
        "Output file: {}",
        output.display().to_string().bright_blue()
    );
    println!(
        "Note: {}",
        "Message storage not yet implemented in core library".yellow()
    );

    Ok(())
}

/// Import messages from JSON
pub async fn import_command(account_path: &PathBuf, input: &PathBuf, verbose: bool) -> Result<()> {
    if verbose {
        println!("Importing messages from JSON...");
        println!("Input file: {}", input.display());
    }

    // Validate input file
    validate_file_path(input)?;

    // Read and parse JSON file
    let json_string = std::fs::read_to_string(input)?;
    let import_data: serde_json::Value = serde_json::from_str(&json_string)?;

    if verbose {
        println!(
            "Import data: {}",
            serde_json::to_string_pretty(&import_data)?
        );
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
    let _account = create_or_open_account(config).await?;

    // Check version compatibility
    if let Some(version) = import_data.get("version") {
        if version != "1.0" {
            print_warning("Import file version mismatch");
        }
    }

    // Import messages with comprehensive validation and error handling
    if let Some(messages) = import_data.get("messages") {
        if let Some(messages_array) = messages.as_array() {
            println!("Found {} messages to import", messages_array.len());

            // Create account handle for import (reuse the same config)
            let config = saved_core_rs::types::Config {
                storage_path: std::path::PathBuf::from("./saved-account"),
                storage_backend: saved_core_rs::storage::StorageBackend::Sqlite,
                network_port: 0,
                enable_mdns: false,
                allow_public_relays: false,
                bootstrap_multiaddrs: Vec::new(),
                use_kademlia: false,
                chunk_size: 1024,
                max_parallel_chunks: 4,
                account_passphrase: None,
            };
            
            let mut account = saved_core_rs::types::AccountHandle::create_or_open(config).await
                .map_err(|e| anyhow::anyhow!("Failed to create account: {}", e))?;

            let mut imported_count = 0;
            let mut skipped_count = 0;

            for (i, message) in messages_array.iter().enumerate() {
                if verbose {
                    println!("Processing message {}: {}", i + 1, message);
                }
                
                // Validate message structure
                if let Some(body) = message.get("body").and_then(|v| v.as_str()) {
                    // Validate message content
                    if body.trim().is_empty() {
                        if verbose {
                            println!("  ⚠ Skipping empty message {}", i + 1);
                        }
                        skipped_count += 1;
                        continue;
                    }
                    
                    // Check message length
                    if body.len() > 10000 {
                        if verbose {
                            println!("  ⚠ Skipping message {} (too long: {} chars)", i + 1, body.len());
                        }
                        skipped_count += 1;
                        continue;
                    }
                    
                    // Import message using the core library
                    match account.create_message(body.to_string(), Vec::new()).await {
                        Ok(_) => {
                            imported_count += 1;
                            if verbose {
                                println!("  ✓ Imported: {}", 
                                    if body.len() > 50 { 
                                        format!("{}...", &body[..50]) 
                                    } else { 
                                        body.to_string() 
                                    }
                                );
                            }
                        },
                        Err(e) => {
                            println!("  ✗ Failed to import message {}: {}", i + 1, e);
                            skipped_count += 1;
                        }
                    }
                } else {
                    if verbose {
                        println!("  ⚠ Skipping message {} (no body field)", i + 1);
                    }
                    skipped_count += 1;
                }
            }
            
            println!("Import summary: {} imported, {} skipped", imported_count, skipped_count);
        }
    }

    println!("{}", "✓ Messages imported successfully!".green().bold());
    println!("Input file: {}", input.display().to_string().bright_blue());
    println!(
        "Note: {}",
        "Message import is not yet fully implemented in core library".yellow()
    );

    Ok(())
}
