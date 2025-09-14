# SAVED Core Library

A private, end-to-end encrypted personal vault that syncs notes, messages, and files across your own devices using P2P networking with libp2p.

## Features

- **P2P by default**: mDNS/LAN → direct → DCUtR → relay fallback
- **End-to-end encryption**: Account-scoped vault key with XChaCha20-Poly1305
- **CRDT semantics**: Append-only event log with conflict-free replicated data types
- **Content-addressed storage**: Encrypted chunk store with deduplication
- **Zero-knowledge**: Optional cloud backup without data exposure
- **No servers required**: Works entirely peer-to-peer

## Architecture

The library implements a CRDT-based sync system with the following components:

- **Cryptography**: ed25519 keys, XChaCha20-Poly1305 encryption, BLAKE3 hashing
- **Storage**: SQLite with WAL mode, content-addressed chunk store
- **Networking**: libp2p with QUIC/TCP, mDNS discovery, DCUtR NAT traversal
- **Sync Protocol**: Gossipsub announcements, Request-Response for operations/chunks

## Quick Start

```rust
use saved_core_rs::{create_or_open_account, Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration
    let config = Config {
        storage_path: std::path::PathBuf::from("./my-account"),
        allow_public_relays: false,
        bootstrap_multiaddrs: Vec::new(),
        use_kademlia: false,
        chunk_size: 2 * 1024 * 1024, // 2 MiB
        max_parallel_chunks: 4,
    };
    
    // Create or open account
    let account = create_or_open_account(config).await?;
    
    // Start network
    account.start_network().await?;
    
    // Create a message
    let message_id = account.create_message(
        "Hello, SAVED!".to_string(),
        Vec::new(), // No attachments
    ).await?;
    
    // Edit the message
    account.edit_message(
        message_id,
        "Hello, SAVED! (edited)".to_string(),
    ).await?;
    
    // Delete the message
    account.delete_message(message_id).await?;
    
    Ok(())
}
```

## Examples

See the `examples/` directory for more detailed usage examples:

- `basic_usage.rs` - Basic message operations
- More examples coming soon...

## Dependencies

The library uses the following key dependencies:

- **libp2p**: P2P networking stack
- **ed25519-dalek**: Cryptographic signatures
- **chacha20poly1305**: Authenticated encryption
- **blake3**: Fast hashing and content addressing
- **rusqlite**: Local database storage
- **prost**: Protocol buffer serialization

## Security

- All content is encrypted with account-scoped keys
- Device authentication via ed25519 certificates
- Convergent encryption for file deduplication
- No global DHT for privacy
- Optional passphrase-protected key backup

## License

MIT OR Apache-2.0

## Status

This is a work in progress. The core library structure is implemented but some features are still being developed:

- ✅ Core data types and configuration
- ✅ Cryptography module (ed25519, XChaCha20-Poly1305, BLAKE3)
- ✅ Storage layer (SQLite, content-addressed chunks)
- ✅ CRDT event log with operations
- ✅ libp2p networking stack
- ✅ Sync protocol implementation
- ✅ Protobuf message definitions
- 🚧 Device linking flow
- 🚧 Complete API implementation
- 🚧 Cloud backup integration

## Contributing

Contributions are welcome! Please see the project repository for contribution guidelines.
