# Simple Two-Peer Test Flow

This is a step-by-step manual test to demonstrate two SAVED peers communicating on the same laptop using the new TCP socket architecture.

## Prerequisites

- SAVED project built (`cargo build`)
- Two terminal windows/tabs

## Step-by-Step Test

### Terminal 1: Start Alice's Daemon

```bash
# Start Alice's daemon on control port 9001
cargo run --bin saved-daemon -- --account-path ./alice --control-port 9001
```

You should see:
```
INFO saved_daemon: Starting SAVED daemon...
INFO saved_daemon: Control server will start on port: 9001
INFO saved_daemon::server: Daemon control server listening on: 127.0.0.1:9001
```

### Terminal 2: Start Bob's Daemon

```bash
# Start Bob's daemon on control port 9002
cargo run --bin saved-daemon -- --account-path ./bob --control-port 9002
```

You should see:
```
INFO saved_daemon: Starting SAVED daemon...
INFO saved_daemon: Control server will start on port: 9002
INFO saved_daemon::server: Daemon control server listening on: 127.0.0.1:9002
```

### Terminal 3: Test Alice's Control

```bash
# Check Alice's status
cargo run --bin savedctl -- --account-path ./alice status

# Alice sends a message
cargo run --bin savedctl -- --account-path ./alice message send "Hello Bob! This is Alice."

# List Alice's messages
cargo run --bin savedctl -- --account-path ./alice message list
```

### Terminal 4: Test Bob's Control

```bash
# Check Bob's status
cargo run --bin savedctl -- --account-path ./bob status

# Bob sends a message
cargo run --bin savedctl -- --account-path ./bob message send "Hi Alice! Nice to meet you via SAVED!"

# List Bob's messages
cargo run --bin savedctl -- --account-path ./bob message list
```

## Expected Results

### Status Commands
Both peers should show:
- Device ID: `local-device`
- Device Name: `Local Device`
- Authorized: `Yes`
- Network Status: `Inactive` (no peers connected yet)
- Total Messages: `1` (after sending messages)

### Message Lists
- Alice should see her message: "Hello Bob! This is Alice."
- Bob should see his message: "Hi Alice! Nice to meet you via SAVED!"

### TCP Communication
- All commands should work via TCP sockets
- No "Daemon not running" fallback messages
- Real-time communication with daemons

## Key Features Demonstrated

✅ **Multiple Daemons**: Two daemons running simultaneously  
✅ **TCP Communication**: Ctl tools connect via TCP sockets  
✅ **Port Separation**: Different control ports (9001, 9002)  
✅ **Message Storage**: Messages stored in separate account databases  
✅ **Real-time Control**: Immediate response to commands  
✅ **Cross-platform**: TCP sockets work on Windows, macOS, Linux  

## Cleanup

```bash
# Stop daemons (Ctrl+C in their terminals)
# Or kill by PID if running in background

# Clean up test data
rm -rf ./alice ./bob
```

## Troubleshooting

### "Daemon not running" message
- Check if daemon is actually running
- Verify control port is correct
- Check `./alice/control.port` or `./bob/control.port` files exist

### Port conflicts
- Use different control ports for each daemon
- Check if ports 9001/9002 are already in use

### Build errors
- Run `cargo build` first
- Check for compilation errors

## Next Steps

This test demonstrates the basic daemon/ctl architecture. For full P2P communication, you would need to implement:
- Peer discovery (mDNS scanning)
- Device authorization
- Message synchronization between peers
- Network connection establishment

The current implementation shows the foundation is solid for building these features on top of the TCP communication layer.
