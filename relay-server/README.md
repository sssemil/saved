# SAVED Public Relay Server

A simple, public relay server for the SAVED messaging system that accepts all traffic and enables hole punching for peer-to-peer connections.

## Features

- **Public Relay**: Accepts all reservation and circuit requests
- **No Restrictions**: No authentication or filtering
- **Hole Punching**: Enables DCUtR (Direct Connection Upgrade Through Relay)
- **Simple Setup**: Easy to run and configure
- **Verbose Logging**: Detailed connection and circuit information

## Usage

### Basic Usage

```bash
# Run on default port 8080
cargo run

# Run on custom port
cargo run -- --port 9090

# Run with verbose logging
cargo run -- --verbose

# Run on specific host and port
cargo run -- --host 0.0.0.0 --port 8080 --verbose
```

### Command Line Options

- `--port, -p`: Port to listen on (default: 8080)
- `--host, -h`: Host to bind to (default: 0.0.0.0)
- `--verbose, -v`: Enable verbose logging

## Relay Address

The server will output a relay address in the format:
```
/ip4/0.0.0.0/tcp/8080/p2p/12D3KooW...
```

This address can be used by SAVED clients to connect through the relay for hole punching.

## How It Works

1. **Reservation**: Peers can reserve slots on the relay server
2. **Circuit Creation**: Peers can create circuits through the relay
3. **Hole Punching**: DCUtR attempts direct connections through the relay
4. **Traffic Relay**: All traffic is relayed without inspection

## Security Note

⚠️ **This is a public relay server with no restrictions!**
- Accepts all reservation requests
- Relays all traffic without filtering
- No authentication required
- Use only for testing and development

## Integration with SAVED

To use this relay server with SAVED clients:

1. Start the relay server
2. Note the relay address (printed on startup)
3. Configure SAVED clients to use the relay address
4. Clients can now connect through the relay for hole punching

## Example Output

```
🚀 SAVED Public Relay Server Started!
📡 Relay Address: /ip4/0.0.0.0/tcp/8080/p2p/12D3KooW...
🔓 Accepting all traffic - no restrictions
⚡ Ready for hole punching connections!

✅ Reservation request accepted from peer: 12D3KooW...
🔗 Circuit established: 12D3KooW... -> 12D3KooW...
```
