//! CRDT event log implementation for SAVED
//!
//! Implements append-only operation log with CRDT semantics for edits/deletes.
//! Operations have IDs, lamport timestamps, and causality tracking.

use crate::crypto::{decrypt, derive_event_key, encrypt, generate_nonce, DeviceKey, VaultKey};
use crate::error::{Error, Result};
use crate::protobuf::*;
use chrono::{DateTime, Utc};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Operation ID: (device_pubkey, counter)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpId {
    /// Device public key (32 bytes)
    pub device_pubkey: [u8; 32],
    /// Monotonic counter per device
    pub counter: u64,
}

impl OpId {
    /// Create a new operation ID
    pub fn new(device_pubkey: [u8; 32], counter: u64) -> Self {
        Self {
            device_pubkey,
            counter,
        }
    }

    /// Serialize to bytes for storage
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(40);
        bytes.extend_from_slice(&self.device_pubkey);
        bytes.extend_from_slice(&self.counter.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 40 {
            return Err(Error::Sync("Invalid OpId length".to_string()));
        }

        let mut device_pubkey = [0u8; 32];
        device_pubkey.copy_from_slice(&bytes[0..32]);

        let mut counter_bytes = [0u8; 8];
        counter_bytes.copy_from_slice(&bytes[32..40]);
        let counter = u64::from_le_bytes(counter_bytes);

        Ok(Self {
            device_pubkey,
            counter,
        })
    }
}

/// Lamport timestamp for logical ordering
pub type LamportTimestamp = u64;

/// Operation hash for referencing
pub type OpHash = [u8; 32];

/// Core operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    CreateMessage {
        msg_id: [u8; 32],
        feed_id: String,
        body: String,
        attachments: Vec<[u8; 32]>, // Chunk CIDs
        created_at: DateTime<Utc>,
    },
    EditMessage {
        msg_id: [u8; 32],
        body: String,
        edited_at: DateTime<Utc>,
    },
    DeleteMessage {
        msg_id: [u8; 32],
        reason: Option<String>,
        deleted_at: DateTime<Utc>,
    },
    Attach {
        msg_id: [u8; 32],
        attachment_cids: Vec<[u8; 32]>,
    },
    Detach {
        msg_id: [u8; 32],
        attachment_cids: Vec<[u8; 32]>,
    },
    Ack {
        up_to_op: OpId,
        timestamp: DateTime<Utc>,
    },
    Purge {
        msg_id: [u8; 32],
        timestamp: DateTime<Utc>,
    },
}

/// Complete operation with metadata
#[derive(Debug, Clone)]
pub struct Op {
    /// Operation ID
    pub id: OpId,
    /// Lamport timestamp
    pub lamport: LamportTimestamp,
    /// Parent operations (causality)
    pub parents: Vec<OpHash>,
    /// The actual operation
    pub operation: Operation,
    /// When this operation was created
    pub timestamp: DateTime<Utc>,
}

impl Op {
    /// Create a new operation
    pub fn new(
        id: OpId,
        lamport: LamportTimestamp,
        parents: Vec<OpHash>,
        operation: Operation,
    ) -> Self {
        Self {
            id,
            lamport,
            parents,
            operation,
            timestamp: Utc::now(),
        }
    }

    /// Compute the hash of this operation
    pub fn hash(&self) -> OpHash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.id.to_bytes());
        hasher.update(&self.lamport.to_le_bytes());
        for parent in &self.parents {
            hasher.update(parent);
        }
        hasher.update(&serde_json::to_vec(&self.operation).unwrap());
        hasher.update(&self.timestamp.timestamp().to_le_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Encrypt the operation for storage/transmission
    pub fn encrypt(&self, vault_key: &VaultKey, device_key: &DeviceKey) -> Result<OpEnvelope> {
        // Serialize the operation
        let operation_bytes = serde_json::to_vec(&self.operation).map_err(Error::Serialization)?;

        // Derive encryption key
        let op_id_bytes = self.id.to_bytes();
        let event_key = derive_event_key(vault_key, &op_id_bytes)?;

        // Generate nonce and encrypt
        let nonce = generate_nonce();
        let ciphertext = encrypt(&event_key, &nonce, &operation_bytes)?;

        // Create header
        let header = OpHeader {
            op_id: op_id_bytes,
            lamport: self.lamport,
            parents: self.parents.iter().map(|h| h.to_vec()).collect(),
            signer: device_key.public_key_bytes().to_vec(),
            sig: Vec::new(), // Will be filled after signing
        };

        // Sign the header and ciphertext
        let mut to_sign = Vec::new();
        to_sign.extend_from_slice(&header.encode_to_vec());
        to_sign.extend_from_slice(&ciphertext);
        let signature = device_key.sign(&to_sign);

        let mut signed_header = header;
        signed_header.sig = signature.to_bytes().to_vec();

        Ok(OpEnvelope {
            header: Some(signed_header),
            ciphertext,
        })
    }

    /// Decrypt and verify an operation envelope
    pub fn decrypt_and_verify(
        envelope: OpEnvelope,
        vault_key: &VaultKey,
        device_key: &DeviceKey,
    ) -> Result<Self> {
        let header = envelope
            .header
            .ok_or_else(|| Error::Sync("Missing operation header".to_string()))?;

        // Verify signature
        let mut to_verify = Vec::new();
        to_verify.extend_from_slice(&header.encode_to_vec());
        to_verify.extend_from_slice(&envelope.ciphertext);

        if header.sig.len() != 64 {
            return Err(Error::Crypto("Invalid signature length".to_string()));
        }
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&header.sig[..]);
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);

        device_key.verify(&to_verify, &signature)?;

        // Decrypt the operation
        let event_key = derive_event_key(vault_key, &header.op_id)?;
        let nonce = generate_nonce(); // TODO: Extract nonce from envelope
        let operation_bytes = decrypt(&event_key, &nonce, &envelope.ciphertext)?;

        let operation: Operation =
            serde_json::from_slice(&operation_bytes).map_err(Error::Serialization)?;

        // Reconstruct the operation
        let id = OpId::from_bytes(&header.op_id)?;
        let parents = header
            .parents
            .iter()
            .map(|p| {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&p[0..32]);
                hash
            })
            .collect();

        Ok(Op {
            id,
            lamport: header.lamport,
            parents,
            operation,
            timestamp: Utc::now(), // TODO: Extract from header
        })
    }
}

/// Event log for managing operations
#[derive(Debug, Clone)]
pub struct EventLog {
    /// All operations indexed by hash
    operations: HashMap<OpHash, Op>,
    /// Operations by device and counter
    by_device: HashMap<[u8; 32], HashMap<u64, OpHash>>,
    /// Current heads (frontier of the DAG)
    heads: HashSet<OpHash>,
    /// Lamport timestamp
    current_lamport: LamportTimestamp,
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLog {
    /// Create a new event log
    pub fn new() -> Self {
        Self {
            operations: HashMap::new(),
            by_device: HashMap::new(),
            heads: HashSet::new(),
            current_lamport: 0,
        }
    }

    /// Add an operation to the log
    pub fn add_operation(&mut self, op: Op) -> Result<OpHash> {
        let hash = op.hash();

        // Check for duplicate
        if self.operations.contains_key(&hash) {
            return Ok(hash);
        }

        // Update lamport timestamp
        self.current_lamport = self.current_lamport.max(op.lamport + 1);

        // Add to device index
        self.by_device
            .entry(op.id.device_pubkey)
            .or_default()
            .insert(op.id.counter, hash);

        // Update heads
        self.heads.remove(&hash); // Remove if it was a head
        for parent in &op.parents {
            self.heads.remove(parent);
        }
        self.heads.insert(hash);

        // Store the operation
        self.operations.insert(hash, op);

        Ok(hash)
    }

    /// Get an operation by hash
    pub fn get_operation(&self, hash: &OpHash) -> Option<&Op> {
        self.operations.get(hash)
    }

    /// Get all current heads
    pub fn get_heads(&self) -> &HashSet<OpHash> {
        &self.heads
    }

    /// Get operations since certain heads
    pub fn get_operations_since(&self, since_heads: &[OpHash]) -> Vec<&Op> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();

        // Start from heads and traverse backwards
        let mut to_visit: Vec<OpHash> = since_heads.to_vec();

        while let Some(hash) = to_visit.pop() {
            if visited.contains(&hash) {
                continue;
            }
            visited.insert(hash);

            if let Some(op) = self.operations.get(&hash) {
                result.push(op);
                to_visit.extend(&op.parents);
            }
        }

        result
    }

    /// Get the current lamport timestamp
    pub fn current_lamport(&self) -> LamportTimestamp {
        self.current_lamport
    }

    /// Get operations for a specific device
    pub fn get_device_operations(&self, device_pubkey: &[u8; 32]) -> Vec<&Op> {
        if let Some(device_ops) = self.by_device.get(device_pubkey) {
            let mut ops: Vec<_> = device_ops
                .values()
                .filter_map(|hash| self.operations.get(hash))
                .collect();
            ops.sort_by_key(|op| op.id.counter);
            ops
        } else {
            Vec::new()
        }
    }

    /// Check if we have all operations up to a certain point
    pub fn has_operations_up_to(&self, up_to: &OpId) -> bool {
        if let Some(device_ops) = self.by_device.get(&up_to.device_pubkey) {
            for counter in 1..=up_to.counter {
                if !device_ops.contains_key(&counter) {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{generate_vault_key, DeviceKey};

    #[test]
    fn test_op_id_serialization() {
        let device_pubkey = [1u8; 32];
        let counter = 42u64;
        let op_id = OpId::new(device_pubkey, counter);

        let bytes = op_id.to_bytes();
        let deserialized = OpId::from_bytes(&bytes).unwrap();

        assert_eq!(op_id, deserialized);
    }

    #[test]
    fn test_event_log_operations() {
        let mut log = EventLog::new();
        let device_key = DeviceKey::generate();
        let vault_key = generate_vault_key();

        let op = Op::new(
            OpId::new(device_key.public_key_bytes(), 1),
            1,
            Vec::new(),
            Operation::CreateMessage {
                msg_id: [1u8; 32],
                feed_id: "default".to_string(),
                body: "Hello, SAVED!".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        );

        let hash = log.add_operation(op).unwrap();
        assert!(log.get_operation(&hash).is_some());
        assert_eq!(log.get_heads().len(), 1);
        assert!(log.get_heads().contains(&hash));
    }
}
