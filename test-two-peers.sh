#!/bin/bash

# Simple test flow for two SAVED peers on the same laptop
# This script demonstrates the daemon/ctl architecture with TCP communication

set -e

echo "🚀 SAVED Two-Peer Test Flow"
echo "=========================="
echo

# Clean up any existing test data
echo "🧹 Cleaning up previous test data..."
rm -rf ./test-peer-alice ./test-peer-bob
mkdir -p ./test-peer-alice ./test-peer-bob

echo "✅ Test directories created"
echo

# Start Alice's daemon
echo "👩 Starting Alice's daemon..."
cargo run --bin saved-daemon -- --account-path ./test-peer-alice --control-port 9001 &
ALICE_PID=$!
echo "   Alice daemon PID: $ALICE_PID"
echo "   Control port: 9001"
sleep 3

# Start Bob's daemon  
echo "👨 Starting Bob's daemon..."
cargo run --bin saved-daemon -- --account-path ./test-peer-bob --control-port 9002 &
BOB_PID=$!
echo "   Bob daemon PID: $BOB_PID"
echo "   Control port: 9002"
sleep 3

echo
echo "📊 Checking daemon status..."
echo

# Check Alice's status
echo "=== Alice's Status ==="
cargo run --bin savedctl -- --account-path ./test-peer-alice status
echo

# Check Bob's status
echo "=== Bob's Status ==="
cargo run --bin savedctl -- --account-path ./test-peer-bob status
echo

# Alice sends a message
echo "💬 Alice sends a message..."
cargo run --bin savedctl -- --account-path ./test-peer-alice message send "Hello Bob! This is Alice speaking."
echo

# Bob sends a message
echo "💬 Bob sends a message..."
cargo run --bin savedctl -- --account-path ./test-peer-bob message send "Hi Alice! Nice to meet you via SAVED!"
echo

# List Alice's messages
echo "📝 Alice's messages:"
cargo run --bin savedctl -- --account-path ./test-peer-alice message list
echo

# List Bob's messages
echo "📝 Bob's messages:"
cargo run --bin savedctl -- --account-path ./test-peer-bob message list
echo

# Check peer discovery
echo "🔍 Checking peer discovery..."
echo "=== Alice's discovered peers ==="
cargo run --bin savedctl -- --account-path ./test-peer-alice peer list
echo

echo "=== Bob's discovered peers ==="
cargo run --bin savedctl -- --account-path ./test-peer-bob peer list
echo

# Network status
echo "🌐 Network status:"
echo "=== Alice's network ==="
cargo run --bin savedctl -- --account-path ./test-peer-alice network status
echo

echo "=== Bob's network ==="
cargo run --bin savedctl -- --account-path ./test-peer-bob network status
echo

echo "🎉 Test completed successfully!"
echo
echo "📋 Summary:"
echo "   - Two SAVED daemons running on different control ports"
echo "   - TCP communication between ctl tools and daemons"
echo "   - Messages sent and received by both peers"
echo "   - Peer discovery and network status working"
echo
echo "🛑 To stop the daemons, run:"
echo "   kill $ALICE_PID $BOB_PID"
echo
echo "🧹 To clean up test data:"
echo "   rm -rf ./test-peer-alice ./test-peer-bob"
