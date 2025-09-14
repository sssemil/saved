# SAVED CLI

A command-line interface for SAVED - Personal Messages Sync. This CLI provides a simple way to interact with the SAVED core library, allowing you to create accounts, manage messages, and sync between devices.

## Installation

```bash
# Build from source
cargo build --release

# Install globally
cargo install --path .
```

## Quick Start

1. **Initialize a new account:**
   ```bash
   saved init --name "My Account"
   ```

2. **Create your first message:**
   ```bash
   saved create --content "Hello, SAVED!"
   ```

3. **List messages:**
   ```bash
   saved list
   ```

4. **Generate QR code for device linking:**
   ```bash
   saved link --output qr-code.png
   ```

5. **Start syncing:**
   ```bash
   saved sync
   ```

## Commands

### Account Management

- `saved init [--name NAME]` - Initialize a new SAVED account
- `saved info` - Show account information

### Message Operations

- `saved create --content TEXT [--attach FILE...]` - Create a new message
- `saved list [--ids-only] [--limit N]` - List all messages
- `saved show MESSAGE_ID` - Show message content
- `saved edit MESSAGE_ID --content TEXT` - Edit a message
- `saved delete MESSAGE_ID [--permanent]` - Delete a message

### Device Linking

- `saved link [--output FILE]` - Generate QR code for device linking
- `saved accept PAYLOAD` - Accept device link from QR code
- `saved devices` - Show connected devices

### Sync & Network

- `saved sync [--daemon]` - Start network sync
- `saved status` - Show sync status

### Import/Export

- `saved export --output FILE` - Export messages to JSON
- `saved import INPUT` - Import messages from JSON

## Examples

### Basic Usage

```bash
# Initialize account
saved init --name "My Personal Vault"

# Create messages
saved create --content "Remember to buy groceries"
saved create --content "Meeting notes from today" --attach notes.txt

# List messages
saved list

# Edit a message
saved edit abc123... --content "Remember to buy groceries and milk"

# Delete a message
saved delete abc123...
```

### Device Linking

```bash
# On device 1: Generate QR code
saved link --output device1-qr.png

# On device 2: Accept link (scan QR code or use payload)
saved accept '{"device_id":"...","addresses":[...],"onboarding_token":"...","expires_at":"..."}'

# Start syncing on both devices
saved sync --daemon
```

### Advanced Usage

```bash
# Export all messages
saved export --output backup.json

# Import messages
saved import backup.json

# Check sync status
saved status

# List only message IDs
saved list --ids-only

# Show limited number of messages
saved list --limit 10
```

## Configuration

The CLI uses the following default configuration:

- **Account path**: `./saved-account`
- **Chunk size**: 2 MiB
- **Max parallel chunks**: 4
- **Public relays**: Disabled
- **Kademlia DHT**: Disabled

You can override the account path using the `--account-path` option:

```bash
saved --account-path /path/to/account init
```

## Output Formats

### Verbose Mode

Use `--verbose` or `-v` for detailed output:

```bash
saved --verbose create --content "Test message"
```

### JSON Output

Some commands support JSON output for scripting:

```bash
saved export --output messages.json
```

## Error Handling

The CLI provides clear error messages and suggestions:

```bash
$ saved show invalid-id
Error: Invalid message ID format
Help: Message ID must be 32 bytes (64 hex characters)

$ saved create --content ""
Error: Message content cannot be empty
```

## Development Status

⚠️ **Note**: This CLI is built on top of the SAVED core library, which is still under development. Some features may not be fully implemented:

- Message storage and retrieval
- Network discovery and connection
- Device authentication
- File attachment handling
- Cloud backup integration

## Contributing

Contributions are welcome! Please see the main SAVED repository for contribution guidelines.

## License

MIT OR Apache-2.0
