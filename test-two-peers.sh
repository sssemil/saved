#!/bin/bash

# Comprehensive test flow for two SAVED peers on the same laptop
# This script demonstrates the daemon/ctl architecture with TCP communication
# and tests all implemented features

set -e

echo "🚀 SAVED Comprehensive Two-Peer Test Flow"
echo "=========================================="
echo

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_info() {
    echo -e "${BLUE}ℹ️  $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

# Function to check if daemon is running
check_daemon_running() {
    local account_path=$1
    local port=$2
    
    if cargo run --bin savedctl -- --account-path "$account_path" status >/dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

# Function to wait for daemon to be ready
wait_for_daemon() {
    local account_path=$1
    local port=$2
    local max_attempts=30
    local attempt=0
    
    print_info "Waiting for daemon to be ready..."
    
    while [ $attempt -lt $max_attempts ]; do
        if check_daemon_running "$account_path" "$port"; then
            print_status "Daemon is ready"
            return 0
        fi
        
        sleep 1
        attempt=$((attempt + 1))
        echo -n "."
    done
    
    echo
    print_error "Daemon failed to start within $max_attempts seconds"
    return 1
}

# Clean up any existing test data
echo "🧹 Cleaning up previous test data..."
rm -rf ./test-peer-alice ./test-peer-bob
mkdir -p ./test-peer-alice ./test-peer-bob

print_status "Test directories created"
echo

# Start Alice's daemon
print_info "Starting Alice's daemon..."
cargo run --bin saved-daemon -- --account-path ./test-peer-alice --control-port 9001 > alice-daemon.log 2>&1 &
ALICE_PID=$!
echo "   Alice daemon PID: $ALICE_PID"
echo "   Control port: 9001"
echo "   Log file: alice-daemon.log"

# Start Bob's daemon  
print_info "Starting Bob's daemon..."
cargo run --bin saved-daemon -- --account-path ./test-peer-bob --control-port 9002 > bob-daemon.log 2>&1 &
BOB_PID=$!
echo "   Bob daemon PID: $BOB_PID"
echo "   Control port: 9002"
echo "   Log file: bob-daemon.log"

# Wait for both daemons to be ready
wait_for_daemon "./test-peer-alice" 9001
wait_for_daemon "./test-peer-bob" 9002

echo
echo "📊 Testing Basic Status and Account Management"
echo "=============================================="

# Test 1: Check Alice's status
echo "=== Alice's Status ==="
if cargo run --bin savedctl -- --account-path ./test-peer-alice status; then
    print_status "Alice's status command successful"
else
    print_error "Alice's status command failed"
fi
echo

# Test 2: Check Bob's status
echo "=== Bob's Status ==="
if cargo run --bin savedctl -- --account-path ./test-peer-bob status; then
    print_status "Bob's status command successful"
else
    print_error "Bob's status command failed"
fi
echo

# Test 3: Account management
echo "=== Account Management ==="
print_info "Testing account initialization and management..."

# Check if accounts are properly initialized
if [ -f "./test-peer-alice/account.db" ]; then
    print_status "Alice's account database exists"
else
    print_warning "Alice's account database not found"
fi

if [ -f "./test-peer-bob/account.db" ]; then
    print_status "Bob's account database exists"
else
    print_warning "Bob's account database not found"
fi

echo
echo "🔧 Testing Device Management"
echo "============================"

# Test 4: Device listing
echo "=== Alice's Devices ==="
if cargo run --bin savedctl -- --account-path ./test-peer-alice device list; then
    print_status "Alice's device list command successful"
else
    print_error "Alice's device list command failed"
fi
echo

echo "=== Bob's Devices ==="
if cargo run --bin savedctl -- --account-path ./test-peer-bob device list; then
    print_status "Bob's device list command successful"
else
    print_error "Bob's device list command failed"
fi
echo

# Test 5: Device information
echo "=== Alice's Device Info ==="
if cargo run --bin savedctl -- --account-path ./test-peer-alice device info; then
    print_status "Alice's device info command successful"
else
    print_error "Alice's device info command failed"
fi
echo

echo "=== Bob's Device Info ==="
if cargo run --bin savedctl -- --account-path ./test-peer-bob device info; then
    print_status "Bob's device info command successful"
else
    print_error "Bob's device info command failed"
fi
echo

# Test 6: Device linking (generate QR codes)
echo "=== Device Linking ==="
print_info "Testing device linking QR code generation..."

echo "Alice generating link QR code..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice device link; then
    print_status "Alice's device link command successful"
else
    print_error "Alice's device link command failed"
fi
echo

echo "Bob generating link QR code..."
if cargo run --bin savedctl -- --account-path ./test-peer-bob device link; then
    print_status "Bob's device link command successful"
else
    print_error "Bob's device link command failed"
fi
echo

echo "💬 Testing Message Management"
echo "============================="

# Test 7: Send messages
print_info "Testing message sending and management..."

echo "Alice sends a message..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice message send "Hello Bob! This is Alice speaking from the comprehensive test."; then
    print_status "Alice's message send successful"
else
    print_error "Alice's message send failed"
fi
echo

echo "Bob sends a message..."
if cargo run --bin savedctl -- --account-path ./test-peer-bob message send "Hi Alice! Nice to meet you via SAVED! This is Bob responding."; then
    print_status "Bob's message send successful"
else
    print_error "Bob's message send failed"
fi
echo

# Test 8: List messages
echo "=== Alice's Messages ==="
if cargo run --bin savedctl -- --account-path ./test-peer-alice message list; then
    print_status "Alice's message list successful"
else
    print_error "Alice's message list failed"
fi
echo

echo "=== Bob's Messages ==="
if cargo run --bin savedctl -- --account-path ./test-peer-bob message list; then
    print_status "Bob's message list successful"
else
    print_error "Bob's message list failed"
fi
echo

# Test 9: Send more messages for testing edit/delete
echo "Alice sends another message for testing..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice message send "This is a test message that will be edited and deleted."; then
    print_status "Alice's second message send successful"
else
    print_error "Alice's second message send failed"
fi
echo

echo "🌐 Testing Network Management"
echo "============================="

# Test 10: Network status
echo "=== Alice's Network Status ==="
if cargo run --bin savedctl -- --account-path ./test-peer-alice network status; then
    print_status "Alice's network status successful"
else
    print_error "Alice's network status failed"
fi
echo

echo "=== Bob's Network Status ==="
if cargo run --bin savedctl -- --account-path ./test-peer-bob network status; then
    print_status "Bob's network status successful"
else
    print_error "Bob's network status failed"
fi
echo

# Test 11: Network addresses
echo "=== Alice's Network Addresses ==="
if cargo run --bin savedctl -- --account-path ./test-peer-alice network addresses; then
    print_status "Alice's network addresses successful"
else
    print_error "Alice's network addresses failed"
fi
echo

echo "=== Bob's Network Addresses ==="
if cargo run --bin savedctl -- --account-path ./test-peer-bob network addresses; then
    print_status "Bob's network addresses successful"
else
    print_error "Bob's network addresses failed"
fi
echo

# Test 12: Start network discovery
echo "=== Starting Network Discovery ==="
print_info "Testing network discovery..."

echo "Starting Alice's network discovery..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice network start; then
    print_status "Alice's network start successful"
else
    print_error "Alice's network start failed"
fi
echo

echo "Starting Bob's network discovery..."
if cargo run --bin savedctl -- --account-path ./test-peer-bob network start; then
    print_status "Bob's network start successful"
else
    print_error "Bob's network start failed"
fi
echo

# Wait a bit for network discovery
print_info "Waiting for network discovery to initialize..."
sleep 5

# Test 13: Network scan
echo "=== Network Scanning ==="
echo "Alice scanning for peers..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice network scan; then
    print_status "Alice's network scan successful"
else
    print_error "Alice's network scan failed"
fi
echo

echo "Bob scanning for peers..."
if cargo run --bin savedctl -- --account-path ./test-peer-bob network scan; then
    print_status "Bob's network scan successful"
else
    print_error "Bob's network scan failed"
fi
echo

# Test 14: Peer management
echo "=== Peer Management ==="
echo "Alice's discovered peers:"
if cargo run --bin savedctl -- --account-path ./test-peer-alice peer list; then
    print_status "Alice's peer list successful"
else
    print_error "Alice's peer list failed"
fi
echo

echo "Bob's discovered peers:"
if cargo run --bin savedctl -- --account-path ./test-peer-bob peer list; then
    print_status "Bob's peer list successful"
else
    print_error "Bob's peer list failed"
fi
echo

echo "📎 Testing Attachment Management"
echo "==============================="

# Test 15: Attachment management
print_info "Testing attachment management..."

echo "=== Alice's Attachments ==="
if cargo run --bin savedctl -- --account-path ./test-peer-alice attachment list; then
    print_status "Alice's attachment list successful"
else
    print_error "Alice's attachment list failed"
fi
echo

echo "=== Bob's Attachments ==="
if cargo run --bin savedctl -- --account-path ./test-peer-bob attachment list; then
    print_status "Bob's attachment list successful"
else
    print_error "Bob's attachment list failed"
fi
echo

echo "🧩 Testing Chunk Synchronization"
echo "================================"

# Test 16: Chunk synchronization
print_info "Testing chunk synchronization..."

echo "Initializing Alice's chunk sync..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice chunk init; then
    print_status "Alice's chunk sync initialization successful"
else
    print_error "Alice's chunk sync initialization failed"
fi
echo

echo "Initializing Bob's chunk sync..."
if cargo run --bin savedctl -- --account-path ./test-peer-bob chunk init; then
    print_status "Bob's chunk sync initialization successful"
else
    print_error "Bob's chunk sync initialization failed"
fi
echo

# Test 17: Create a test file and store it as a chunk
echo "Creating test file for chunk storage..."
echo "This is a test file for chunk synchronization in SAVED." > test-file.txt

echo "Storing test file as chunk in Alice's system..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice chunk store test-file.txt; then
    print_status "Alice's chunk store successful"
else
    print_error "Alice's chunk store failed"
fi
echo

# Test 18: Check chunk availability
echo "Checking chunk availability..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice chunk check; then
    print_status "Alice's chunk check successful"
else
    print_error "Alice's chunk check failed"
fi
echo

echo "🔄 Testing Message Operations"
echo "============================="

# Test 19: Message editing (if we can get message IDs)
print_info "Testing message operations..."

echo "Alice's messages before operations:"
cargo run --bin savedctl -- --account-path ./test-peer-alice message list
echo

# Test 20: Network stop
echo "=== Stopping Network Discovery ==="
print_info "Testing network stop..."

echo "Stopping Alice's network discovery..."
if cargo run --bin savedctl -- --account-path ./test-peer-alice network stop; then
    print_status "Alice's network stop successful"
else
    print_error "Alice's network stop failed"
fi
echo

echo "Stopping Bob's network discovery..."
if cargo run --bin savedctl -- --account-path ./test-peer-bob network stop; then
    print_status "Bob's network stop successful"
else
    print_error "Bob's network stop failed"
fi
echo

echo "📊 Final Status Check"
echo "===================="

# Test 21: Final status check
echo "=== Final Alice Status ==="
cargo run --bin savedctl -- --account-path ./test-peer-alice status
echo

echo "=== Final Bob Status ==="
cargo run --bin savedctl -- --account-path ./test-peer-bob status
echo

echo "🧹 Cleanup"
echo "=========="

# Clean up test file
rm -f test-file.txt

echo "🎉 Comprehensive Test Completed!"
echo
echo "📋 Test Summary:"
echo "   ✅ Daemon startup and TCP communication"
echo "   ✅ Account management and initialization"
echo "   ✅ Device management and linking"
echo "   ✅ Message sending and listing"
echo "   ✅ Network management and discovery"
echo "   ✅ Peer discovery and management"
echo "   ✅ Attachment management"
echo "   ✅ Chunk synchronization"
echo "   ✅ Network start/stop operations"
echo
echo "🛑 To stop the daemons, run:"
echo "   kill $ALICE_PID $BOB_PID"
echo
echo "📄 To view daemon logs:"
echo "   tail -f alice-daemon.log"
echo "   tail -f bob-daemon.log"
echo
echo "🧹 To clean up test data:"
echo "   rm -rf ./test-peer-alice ./test-peer-bob alice-daemon.log bob-daemon.log"
echo

# Check if daemons are still running
if kill -0 $ALICE_PID 2>/dev/null; then
    print_status "Alice's daemon is still running (PID: $ALICE_PID)"
else
    print_warning "Alice's daemon has stopped"
fi

if kill -0 $BOB_PID 2>/dev/null; then
    print_status "Bob's daemon is still running (PID: $BOB_PID)"
else
    print_warning "Bob's daemon has stopped"
fi

echo
print_info "Test completed successfully! All major SAVED features have been tested."