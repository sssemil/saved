# Running Multiple SAVED Daemons on the Same Machine

This guide explains how to run multiple SAVED daemons on the same machine, each with their own account and network configuration.

## Overview

Each SAVED daemon instance requires:
- **Unique account path**: Each daemon needs its own account directory
- **Random port assignment**: Daemons automatically use random ports to avoid conflicts
- **Control port**: Each daemon runs a TCP control server for `savedctl` communication
- **Separate storage**: Each account has its own SQLite database and chunk storage

### Communication Architecture

- **Daemon**: Runs background network management and storage
- **Control Server**: TCP server (default random port) for `savedctl` communication
- **Ctl Tool**: Connects to daemon via TCP sockets for real-time control
- **Port Discovery**: Control port saved in `./account/control.port` file

## Basic Setup

### 1. Create Separate Account Directories

```bash
# Create directories for different accounts
mkdir -p ~/saved-accounts/alice
mkdir -p ~/saved-accounts/bob
mkdir -p ~/saved-accounts/charlie
```

### 2. Start Multiple Daemons

```bash
# Terminal 1: Alice's daemon
saved-daemon --account-path ~/saved-accounts/alice &

# Terminal 2: Bob's daemon  
saved-daemon --account-path ~/saved-accounts/bob &

# Terminal 3: Charlie's daemon
saved-daemon --account-path ~/saved-accounts/charlie &
```

### 3. Control Each Daemon

```bash
# Check Alice's status
savedctl --account-path ~/saved-accounts/alice status

# Check Bob's status
savedctl --account-path ~/saved-accounts/bob status

# Send message from Alice
savedctl --account-path ~/saved-accounts/alice message send "Hello from Alice!"

# List Bob's messages
savedctl --account-path ~/saved-accounts/bob message list
```

## Advanced Configuration

### Using Different Network and Control Ports (Optional)

While daemons use random ports by default, you can specify fixed ports:

```bash
# Alice on network port 8001, control port 9001
saved-daemon --account-path ~/saved-accounts/alice --network-port 8001 --control-port 9001 &

# Bob on network port 8002, control port 9002
saved-daemon --account-path ~/saved-accounts/bob --network-port 8002 --control-port 9002 &

# Charlie on random ports (default)
saved-daemon --account-path ~/saved-accounts/charlie &
```

### Using Passphrases

```bash
# Alice with passphrase
saved-daemon --account-path ~/saved-accounts/alice --passphrase "alice-secret" &

# Bob with different passphrase
saved-daemon --account-path ~/saved-accounts/bob --passphrase "bob-secret" &
```

### Custom Log Levels

```bash
# Alice with debug logging
saved-daemon --account-path ~/saved-accounts/alice --log-level debug &

# Bob with error-only logging
saved-daemon --account-path ~/saved-accounts/bob --log-level error &
```

## Process Management

### Using systemd (Recommended for Production)

Create service files for each daemon:

```bash
# /etc/systemd/system/saved-alice.service
[Unit]
Description=SAVED Daemon - Alice
After=network.target

[Service]
Type=simple
User=alice
WorkingDirectory=/home/alice
ExecStart=/usr/local/bin/saved-daemon --account-path /home/alice/saved-account
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
# /etc/systemd/system/saved-bob.service
[Unit]
Description=SAVED Daemon - Bob
After=network.target

[Service]
Type=simple
User=bob
WorkingDirectory=/home/bob
ExecStart=/usr/local/bin/saved-daemon --account-path /home/bob/saved-account
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Enable and start services:

```bash
sudo systemctl enable saved-alice
sudo systemctl enable saved-bob
sudo systemctl start saved-alice
sudo systemctl start saved-bob
```

### Using Docker

```bash
# Alice's container
docker run -d --name saved-alice \
  -v ~/saved-accounts/alice:/app/account \
  -p 8001:8001 \
  saved-daemon --account-path /app/account --network-port 8001

# Bob's container
docker run -d --name saved-bob \
  -v ~/saved-accounts/bob:/app/account \
  -p 8002:8002 \
  saved-daemon --account-path /app/account --network-port 8002
```

### Using tmux/screen

```bash
# Create tmux sessions for each daemon
tmux new-session -d -s saved-alice 'saved-daemon --account-path ~/saved-accounts/alice'
tmux new-session -d -s saved-bob 'saved-daemon --account-path ~/saved-accounts/bob'
tmux new-session -d -s saved-charlie 'saved-daemon --account-path ~/saved-accounts/charlie'

# Attach to sessions
tmux attach -t saved-alice
tmux attach -t saved-bob
tmux attach -t saved-charlie
```

## Monitoring Multiple Daemons

### Check All Daemon Status

```bash
#!/bin/bash
# check-all-daemons.sh

ACCOUNTS=("alice" "bob" "charlie")

for account in "${ACCOUNTS[@]}"; do
    echo "=== $account ==="
    savedctl --account-path ~/saved-accounts/$account status
    echo
done
```

### Send Messages to All Accounts

```bash
#!/bin/bash
# broadcast-message.sh

MESSAGE="$1"
ACCOUNTS=("alice" "bob" "charlie")

for account in "${ACCOUNTS[@]}"; do
    echo "Sending to $account..."
    savedctl --account-path ~/saved-accounts/$account message send "$MESSAGE"
done
```

## Network Discovery Between Daemons

### Local Network Discovery

All daemons on the same machine will automatically discover each other via mDNS:

```bash
# Check discovered peers from Alice's perspective
savedctl --account-path ~/saved-accounts/alice peer list

# Should show Bob and Charlie as discovered peers
```

### Manual Connection

```bash
# Alice connects to Bob (if you have Bob's device ID)
savedctl --account-path ~/saved-accounts/alice peer connect <bob-device-id>
```

## Troubleshooting

### Port Conflicts

If you encounter port conflicts:

```bash
# Check what ports are in use
netstat -tulpn | grep saved-daemon

# Kill conflicting processes
pkill -f saved-daemon

# Restart with explicit ports
saved-daemon --account-path ~/saved-accounts/alice --network-port 8001 &
saved-daemon --account-path ~/saved-accounts/bob --network-port 8002 &
```

### Account Path Issues

```bash
# Ensure account directories exist and are writable
mkdir -p ~/saved-accounts/alice
chmod 755 ~/saved-accounts/alice

# Check account status
savedctl --account-path ~/saved-accounts/alice status
```

### Log Analysis

```bash
# View daemon logs (if using systemd)
journalctl -u saved-alice -f
journalctl -u saved-bob -f

# View logs from background processes
ps aux | grep saved-daemon
```

## Best Practices

1. **Use separate user accounts** for different SAVED instances in production
2. **Set up proper logging** with log rotation
3. **Monitor resource usage** - each daemon uses memory and CPU
4. **Use systemd or Docker** for production deployments
5. **Backup account directories** regularly
6. **Use passphrases** for sensitive accounts
7. **Monitor network connectivity** between daemons

## Example: Development Setup

For development with multiple test accounts:

```bash
# Create test accounts
mkdir -p ./test-accounts/{alice,bob,charlie}

# Start test daemons
saved-daemon --account-path ./test-accounts/alice --log-level debug &
saved-daemon --account-path ./test-accounts/bob --log-level debug &
saved-daemon --account-path ./test-accounts/charlie --log-level debug &

# Test messaging between accounts
savedctl --account-path ./test-accounts/alice message send "Hello from Alice!"
savedctl --account-path ./test-accounts/bob message send "Hello from Bob!"

# Check all accounts
for account in alice bob charlie; do
    echo "=== $account ==="
    savedctl --account-path ./test-accounts/$account message list
done
```

This setup allows you to test P2P messaging, device authorization, and network discovery between multiple SAVED instances on the same machine.
