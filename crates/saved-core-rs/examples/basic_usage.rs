//! Basic usage example for the SAVED core library
//!
//! This example demonstrates how to:
//! - Create or open an account
//! - Create messages
//! - Edit and delete messages
//! - Handle events

use saved_core_rs::{create_or_open_account, Config, Event};
use std::path::PathBuf;
use std::sync::mpsc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("SAVED Core Library - Basic Usage Example");
    
    // Create configuration
    let config = Config {
        storage_path: PathBuf::from("./example-account"),
        allow_public_relays: false,
        bootstrap_multiaddrs: Vec::new(),
        use_kademlia: false,
        chunk_size: 2 * 1024 * 1024, // 2 MiB
        max_parallel_chunks: 4,
    };
    
    // Create or open account
    println!("Creating/opening account...");
    let account = create_or_open_account(config).await?;
    
    // Get device info
    let device_info = account.device_info().await;
    println!("Device info: {:?}", device_info);
    
    // Subscribe to events
    let event_receiver = account.subscribe().await;
    
    // Start network (this would normally be done in a separate task)
    println!("Starting network...");
    account.start_network().await?;
    
    // Create a message
    println!("Creating a message...");
    let message_id = account.create_message(
        "Hello, SAVED! This is my first message.".to_string(),
        Vec::new(), // No attachments
    ).await?;
    println!("Created message with ID: {:?}", message_id);
    
    // Wait a bit for events
    sleep(Duration::from_millis(100)).await;
    
    // Edit the message
    println!("Editing the message...");
    account.edit_message(
        message_id,
        "Hello, SAVED! This is my first message (edited).".to_string(),
    ).await?;
    println!("Message edited successfully");
    
    // Wait a bit for events
    sleep(Duration::from_millis(100)).await;
    
    // Delete the message
    println!("Deleting the message...");
    account.delete_message(message_id).await?;
    println!("Message deleted successfully");
    
    // Wait a bit for events
    sleep(Duration::from_millis(100)).await;
    
    // Check for events
    println!("Checking for events...");
    while let Ok(event) = event_receiver.try_recv() {
        match event {
            Event::Connected(device_info) => {
                println!("Device connected: {:?}", device_info);
            }
            Event::Disconnected(peer_id) => {
                println!("Device disconnected: {}", peer_id);
            }
            Event::HeadsUpdated => {
                println!("Heads updated - sync in progress");
            }
            Event::SyncProgress { done, total } => {
                println!("Sync progress: {}/{}", done, total);
            }
            Event::MessageReceived(msg_id) => {
                println!("Message received: {:?}", msg_id);
            }
            Event::MessageEdited(msg_id) => {
                println!("Message edited: {:?}", msg_id);
            }
            Event::MessageDeleted(msg_id) => {
                println!("Message deleted: {:?}", msg_id);
            }
        }
    }
    
    println!("Example completed successfully!");
    Ok(())
}
