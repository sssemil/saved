//! Protobuf message definitions for the SAVED protocol

use prost::Message;
use serde::{Deserialize, Serialize};

/// Operation header containing metadata and signature
#[derive(Clone, PartialEq, Message)]
pub struct OpHeader {
    /// 40 bytes: 32 pubkey hash + 8 counter
    #[prost(bytes, tag = "1")]
    pub op_id: Vec<u8>,
    /// Logical time
    #[prost(uint64, tag = "2")]
    pub lamport: u64,
    /// Operation hashes that this operation depends on
    #[prost(bytes, repeated, tag = "3")]
    pub parents: Vec<Vec<u8>>,
    /// Device public key that signed this operation
    #[prost(bytes, tag = "4")]
    pub signer: Vec<u8>,
    /// Signature over (header || ciphertext)
    #[prost(bytes, tag = "5")]
    pub sig: Vec<u8>,
}

/// Create message operation body
#[derive(Clone, PartialEq, Message)]
pub struct CreateMessageBody {
    /// 32-byte message ID hash
    #[prost(bytes, tag = "1")]
    pub msg_id: Vec<u8>,
    /// Feed identifier (default: "default")
    #[prost(string, tag = "2")]
    pub feed_id: String,
    /// Message body (plaintext before encryption)
    #[prost(string, tag = "3")]
    pub body: String,
    /// Attachment chunk CIDs
    #[prost(bytes, repeated, tag = "4")]
    pub att_cids: Vec<Vec<u8>>,
    /// Wall clock timestamp for UX
    #[prost(int64, tag = "5")]
    pub created_at_ms: i64,
}

/// Edit message operation body
#[derive(Clone, PartialEq, Message)]
pub struct EditMessageBody {
    /// Message ID to edit
    #[prost(bytes, tag = "1")]
    pub msg_id: Vec<u8>,
    /// Either full body replacement or delta
    #[prost(oneof = "edit_message_body::Payload", tags = "2, 3")]
    pub payload: Option<edit_message_body::Payload>,
    /// Wall clock timestamp for UX
    #[prost(int64, tag = "4")]
    pub edited_at_ms: i64,
}

pub mod edit_message_body {
    use prost::Oneof;

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Payload {
        /// Full body replacement
        #[prost(string, tag = "2")]
        BodyFull(String),
        /// Delta for bandwidth saving (reserved for future)
        #[prost(bytes, tag = "3")]
        BodyDelta(Vec<u8>),
    }
}

/// Delete message operation body
#[derive(Clone, PartialEq, Message)]
pub struct DeleteMessageBody {
    /// Message ID to delete
    #[prost(bytes, tag = "1")]
    pub msg_id: Vec<u8>,
    /// Optional reason for deletion
    #[prost(string, tag = "2")]
    pub reason: String,
    /// Wall clock timestamp for UX
    #[prost(int64, tag = "3")]
    pub deleted_at_ms: i64,
}

/// Attach files to message operation body
#[derive(Clone, PartialEq, Message)]
pub struct AttachBody {
    /// Message ID to attach files to
    #[prost(bytes, tag = "1")]
    pub msg_id: Vec<u8>,
    /// Chunk CIDs to attach
    #[prost(bytes, repeated, tag = "2")]
    pub att_cids: Vec<Vec<u8>>,
}

/// Detach files from message operation body
#[derive(Clone, PartialEq, Message)]
pub struct DetachBody {
    /// Message ID to detach files from
    #[prost(bytes, tag = "1")]
    pub msg_id: Vec<u8>,
    /// Chunk CIDs to detach
    #[prost(bytes, repeated, tag = "2")]
    pub att_cids: Vec<Vec<u8>>,
}

/// Acknowledge operations up to a certain point (for compaction)
#[derive(Clone, PartialEq, Message)]
pub struct AckBody {
    /// Operation ID to acknowledge up to
    #[prost(bytes, tag = "1")]
    pub up_to_op: Vec<u8>,
    /// Wall clock timestamp
    #[prost(int64, tag = "2")]
    pub ts: i64,
}

/// Purge message operation body (hard delete)
#[derive(Clone, PartialEq, Message)]
pub struct PurgeBody {
    /// Message ID to purge
    #[prost(bytes, tag = "1")]
    pub msg_id: Vec<u8>,
    /// Wall clock timestamp
    #[prost(int64, tag = "2")]
    pub ts: i64,
}

/// Operation envelope containing header and encrypted payload
#[derive(Clone, PartialEq, Message)]
pub struct OpEnvelope {
    /// Operation header with metadata and signature
    #[prost(message, tag = "1")]
    pub header: Option<OpHeader>,
    /// AEAD-encrypted serialized operation body
    #[prost(bytes, tag = "2")]
    pub ciphertext: Vec<u8>,
}

/// Gossipsub message for announcing new heads
#[derive(Clone, PartialEq, Message)]
pub struct AnnounceHeads {
    /// Feed identifier
    #[prost(string, tag = "1")]
    pub feed_id: String,
    /// Latest lamport timestamp
    #[prost(uint64, tag = "2")]
    pub lamport: u64,
    /// Frontier set of operation hashes
    #[prost(bytes, repeated, tag = "3")]
    pub heads: Vec<Vec<u8>>,
}

/// Request to fetch operations since certain heads
#[derive(Clone, PartialEq, Message)]
pub struct FetchOpsReq {
    /// Head hashes to fetch operations since
    #[prost(bytes, repeated, tag = "1")]
    pub since_heads: Vec<Vec<u8>>,
    /// Maximum number of operations to return
    #[prost(uint32, tag = "2")]
    pub want_max: u32,
}

/// Response containing requested operations
#[derive(Clone, PartialEq, Message)]
pub struct FetchOpsResp {
    /// Requested operations
    #[prost(message, repeated, tag = "1")]
    pub ops: Vec<OpEnvelope>,
    /// New head hashes discovered
    #[prost(bytes, repeated, tag = "2")]
    pub new_heads: Vec<Vec<u8>>,
}

/// Request to check which chunks a peer has
#[derive(Clone, PartialEq, Message)]
pub struct HaveChunksReq {
    /// Chunk CIDs to check
    #[prost(bytes, repeated, tag = "1")]
    pub cids: Vec<Vec<u8>>,
}

/// Response indicating which chunks are available
#[derive(Clone, PartialEq, Message)]
pub struct HaveChunksResp {
    /// Bitmap indicating which chunks are available (1 = have, 0 = don't have)
    #[prost(bytes, tag = "1")]
    pub have_bitmap: Vec<u8>,
}

/// Request to fetch specific chunks
#[derive(Clone, PartialEq, Message)]
pub struct FetchChunksReq {
    /// Chunk CIDs to fetch
    #[prost(bytes, repeated, tag = "1")]
    pub cids: Vec<Vec<u8>>,
}

/// Response containing requested chunks
#[derive(Clone, PartialEq, Message)]
pub struct FetchChunksResp {
    /// Chunk data
    #[prost(message, repeated, tag = "1")]
    pub chunks: Vec<fetch_chunks_resp::Chunk>,
}

pub mod fetch_chunks_resp {
    use prost::Message;

    /// Individual chunk in fetch response
    #[derive(Clone, PartialEq, Message)]
    pub struct Chunk {
        /// Chunk CID
        #[prost(bytes, tag = "1")]
        pub cid: Vec<u8>,
        /// Encrypted chunk data
        #[prost(bytes, tag = "2")]
        pub data: Vec<u8>,
    }
}

/// Device linking onboarding token
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct OnboardingToken {
    /// Account public key
    #[prost(bytes, tag = "1")]
    pub account_pubkey: Vec<u8>,
    /// Token expiration timestamp
    #[prost(int64, tag = "2")]
    pub expires_at: i64,
    /// Random nonce for uniqueness
    #[prost(bytes, tag = "3")]
    pub nonce: Vec<u8>,
    /// Signature by account key
    #[prost(bytes, tag = "4")]
    pub signature: Vec<u8>,
}

/// Protocol identifiers
pub mod protocol {
    /// Gossipsub topic for head announcements
    pub const HEADS_TOPIC: &str = "/savedmsgs/{account_id}/heads";
    
    /// Request-response protocol for operations
    pub const OPS_PROTOCOL: &str = "/savedmsgs/1/ops";
    
    /// Request-response protocol for chunks
    pub const CHUNKS_PROTOCOL: &str = "/savedmsgs/1/chunks";
    
    /// Request-response protocol for device linking
    pub const LINKING_PROTOCOL: &str = "/savedmsgs/1/linking";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_header_serialization() {
        let header = OpHeader {
            op_id: vec![1, 2, 3, 4],
            lamport: 42,
            parents: vec![vec![5, 6, 7, 8]],
            signer: vec![9, 10, 11, 12],
            sig: vec![13, 14, 15, 16],
        };
        
        let encoded = header.encode_to_vec();
        let decoded = OpHeader::decode(&encoded[..]).unwrap();
        
        assert_eq!(header, decoded);
    }

    #[test]
    fn test_create_message_serialization() {
        let body = CreateMessageBody {
            msg_id: vec![1, 2, 3, 4],
            feed_id: "default".to_string(),
            body: "Hello, SAVED!".to_string(),
            att_cids: vec![vec![5, 6, 7, 8]],
            created_at_ms: 1234567890,
        };
        
        let encoded = body.encode_to_vec();
        let decoded = CreateMessageBody::decode(&encoded[..]).unwrap();
        
        assert_eq!(body, decoded);
    }
}
