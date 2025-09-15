# SAVED Project TODOs

## 🚧 **Major Missing Components**

### **1. Real Networking Implementation**
- **Current State**: Simplified placeholder networking with simulated connections
- **Missing**: 
  - Full `libp2p` integration (currently commented out due to API changes)
  - Real mDNS discovery
  - DCUtR hole punching for NAT traversal
  - Relay fallback connections
  - Actual peer-to-peer message synchronization

### **2. Message Storage & Retrieval**
- **Current State**: SQLite-backed message storage implemented with CRUD and listing
- **Missing**:
  - Attachment metadata and retrieval APIs
  - CRDT-based message synchronization

### **3. Device Authentication & Authorization**
- **Current State**: Basic certificate structure exists but not fully implemented
- **Missing**:
  - Complete device linking flow
  - QR code-based device pairing
  - Device certificate validation
  - Device authorization management

### **4. File Attachment System**
- **Current State**: Chunk storage implemented with content addressing and ref counts
- **Missing**:
  - Attachment metadata management
  - File deduplication

### **5. Event Processing & CRDT Logic**
- **Current State**: Event structures exist but processing is incomplete
- **Missing**:
  - Real CRDT conflict resolution
  - Event log synchronization
  - Operation ordering and causality
  - Message edit/deletion convergence

## 🔧 **Technical Debt & Improvements Needed**

### **1. Networking Stack**
```rust
// Current: Simplified placeholder
pub async fn connect_to_peer(&mut self, peer_id: &str) -> Result<()> {
    // Simulate connection
}

// Needed: Real libp2p integration
```

### **2. Storage Implementation**
```rust
// Current: Placeholder message storage
"messages": [], // Empty array in export
"note": "Message storage not yet implemented in core library"

// Needed: Real SQLite message persistence
```

### **3. CLI Functionality**
- Import/export commands show "not yet implemented" messages
- Device discovery shows "not yet implemented"
- Sync status shows "N/A" for most metrics

## 📋 **Priority Implementation Order**

### **Phase 1: Core Storage (High Priority)**
1. **Message Storage**: Implement real message persistence in SQLite (done)
2. **File Attachments**: Implement file chunking and storage (done)
3. **Event Log**: Complete CRDT event processing

### **Phase 2: Networking (High Priority)**
1. **libp2p Integration**: Update to current libp2p API
2. **Device Discovery**: Implement mDNS and manual peer discovery
3. **Connection Management**: Real peer connections and message sync

### **Phase 3: Device Management (Medium Priority)**
1. **Device Linking**: Complete QR code-based pairing
2. **Authentication**: Full device certificate validation
3. **Authorization**: Device permission management

### **Phase 4: Advanced Features (Lower Priority)**
1. **NAT Traversal**: DCUtR hole punching
2. **Relay Fallback**: Relay server integration
3. **Performance**: Optimization and caching

## 🎯 **Immediate Next Steps**

The most critical missing piece is **real message storage and retrieval**. The CLI commands exist but don't actually persist data. This should be the first priority, followed by basic networking to enable device-to-device communication.

## 📝 **Specific TODOs by File**

### **crates/saved-core-rs/src/storage/sqlite.rs**
- [x] Implement real message storage in SQLite
- [x] Add message retrieval methods
- [x] Implement file attachment storage
- [ ] Add CRDT event log persistence

### **crates/saved-core-rs/src/sync.rs**
- [x] Load persisted operations into event log on startup

### **crates/saved-core-rs/src/lib.rs**
- [x] Add persistence test to ensure operations/messages survive restart (SQLite)

### **crates/saved-core-rs/src/networking.rs**
- [ ] Update libp2p integration to current API
- [ ] Implement real mDNS discovery
- [ ] Add DCUtR hole punching
- [ ] Implement relay fallback connections
- [ ] Add real peer-to-peer message sync

### **crates/saved-core-rs/src/sync.rs**
- [ ] Implement real CRDT conflict resolution
- [ ] Add event log synchronization
- [ ] Implement operation ordering and causality
- [ ] Add message edit/deletion convergence

### **crates/saved-core-rs/src/types.rs**
- [ ] Complete device linking flow
- [ ] Implement QR code-based device pairing
- [ ] Add device certificate validation
- [ ] Implement device authorization management

### **crates/saved-cli-rs/src/commands/**
- [x] Fix import/export to work with real message storage
- [ ] Implement real device discovery
- [x] Add proper sync status reporting
- [ ] Implement file attachment handling

### **CLI Discovery**
- [x] Add `discover` command to show mDNS/manual discovered peers

## 🔍 **Current Status Summary**

- ✅ **Build**: Successful with only minor warnings
- ✅ **Tests**: Core storage tests added and passing
- ✅ **Code Quality**: Improved with better integration of utility functions
- ✅ **CLI Enhancement**: Better user experience with consistent formatting and validation
- 🚧 **Core Functionality**: Placeholder implementations need real functionality
- 🚧 **Networking**: Simplified implementation needs full libp2p integration
- 🚧 **Storage**: CRDT event persistence outstanding
- 🚧 **Device Management**: Authentication and linking incomplete

## 📊 **Progress Tracking**

- [ ] Phase 1: Core Storage (3/3 completed)
- [ ] Phase 2: Networking (0/3 completed)
- [ ] Phase 3: Device Management (0/3 completed)
- [ ] Phase 4: Advanced Features (0/3 completed)

**Overall Progress**: 0/12 major components completed
