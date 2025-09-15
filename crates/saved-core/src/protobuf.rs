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
    /// Unix timestamp when operation was created
    #[prost(int64, tag = "6")]
    pub timestamp: i64,
    /// 24-byte nonce for encryption
    #[prost(bytes, tag = "7")]
    pub nonce: Vec<u8>,
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

/// Request attachment metadata from a peer
#[derive(Clone, PartialEq, Message)]
pub struct RequestAttachmentMetadataReq {
    /// Message IDs to get attachment metadata for
    #[prost(bytes, repeated, tag = "1")]
    pub message_ids: Vec<Vec<u8>>,
    /// Optional: specific attachment IDs to request
    #[prost(int64, repeated, tag = "2")]
    pub attachment_ids: Vec<i64>,
}

/// Response with attachment metadata
#[derive(Clone, PartialEq, Message)]
pub struct RequestAttachmentMetadataResp {
    /// List of attachment metadata
    #[prost(message, repeated, tag = "1")]
    pub attachments: Vec<request_attachment_metadata_resp::AttachmentMetadata>,
}

pub mod request_attachment_metadata_resp {
    use prost::Message;
    
    /// Attachment metadata for synchronization
    #[derive(Clone, PartialEq, Message)]
    pub struct AttachmentMetadata {
        /// Attachment ID
        #[prost(int64, tag = "1")]
        pub id: i64,
        /// Message ID this attachment belongs to
        #[prost(bytes, tag = "2")]
        pub message_id: Vec<u8>,
        /// Filename
        #[prost(string, tag = "3")]
        pub filename: String,
        /// File size in bytes
        #[prost(uint64, tag = "4")]
        pub size: u64,
        /// BLAKE3 hash of entire file for deduplication
        #[prost(bytes, tag = "5")]
        pub file_hash: Vec<u8>,
        /// MIME type
        #[prost(string, optional, tag = "6")]
        pub mime_type: Option<String>,
        /// Attachment status (0=Active, 1=Deleted, 2=Purged)
        #[prost(int32, tag = "7")]
        pub status: i32,
        /// Creation timestamp
        #[prost(int64, tag = "8")]
        pub created_at: i64,
        /// Last accessed timestamp (optional)
        #[prost(int64, optional, tag = "9")]
        pub last_accessed: Option<i64>,
        /// Chunk IDs for this attachment
        #[prost(bytes, repeated, tag = "10")]
        pub chunk_ids: Vec<Vec<u8>>,
    }
}

/// Announce attachment metadata to peers
#[derive(Clone, PartialEq, Message)]
pub struct AnnounceAttachmentMetadata {
    /// List of attachment metadata to announce
    #[prost(message, repeated, tag = "1")]
    pub attachments: Vec<announce_attachment_metadata::AttachmentMetadata>,
}

pub mod announce_attachment_metadata {
    use prost::Message;
    
    /// Attachment metadata for announcement
    #[derive(Clone, PartialEq, Message)]
    pub struct AttachmentMetadata {
        /// Attachment ID
        #[prost(int64, tag = "1")]
        pub id: i64,
        /// Message ID this attachment belongs to
        #[prost(bytes, tag = "2")]
        pub message_id: Vec<u8>,
        /// Filename
        #[prost(string, tag = "3")]
        pub filename: String,
        /// File size in bytes
        #[prost(uint64, tag = "4")]
        pub size: u64,
        /// BLAKE3 hash of entire file for deduplication
        #[prost(bytes, tag = "5")]
        pub file_hash: Vec<u8>,
        /// MIME type
        #[prost(string, optional, tag = "6")]
        pub mime_type: Option<String>,
        /// Attachment status (0=Active, 1=Deleted, 2=Purged)
        #[prost(int32, tag = "7")]
        pub status: i32,
        /// Creation timestamp
        #[prost(int64, tag = "8")]
        pub created_at: i64,
        /// Chunk IDs for this attachment (for availability checking)
        #[prost(bytes, repeated, tag = "9")]
        pub chunk_ids: Vec<Vec<u8>>,
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

/// Main SAVED protocol message wrapper
#[derive(Clone, PartialEq, Message)]
pub struct SavedMessage {
    #[prost(oneof = "saved_message::MessageType", tags = "1, 2, 3, 4, 5, 6, 7, 8")]
    pub message_type: Option<saved_message::MessageType>,
}

/// SAVED message types
pub mod saved_message {
    use super::*;
    
    #[derive(Clone, PartialEq, Message)]
    pub enum MessageType {
        #[prost(message, tag = "1")]
        AnnounceHeads(super::SavedAnnounceHeads),
        #[prost(message, tag = "2")]
        FetchOpsReq(super::SavedFetchOpsReq),
        #[prost(message, tag = "3")]
        FetchOpsResp(super::SavedFetchOpsResp),
        #[prost(message, tag = "4")]
        HaveChunksReq(super::SavedHaveChunksReq),
        #[prost(message, tag = "5")]
        HaveChunksResp(super::SavedHaveChunksResp),
        #[prost(message, tag = "6")]
        FetchChunksReq(super::SavedFetchChunksReq),
        #[prost(message, tag = "7")]
        FetchChunksResp(super::SavedFetchChunksResp),
        #[prost(message, tag = "8")]
        Ack(super::SavedAck),
    }
}

/// Announce heads message for CRDT synchronization
#[derive(Clone, PartialEq, Message)]
pub struct SavedAnnounceHeads {
    /// Current head operation hashes
    #[prost(bytes, repeated, tag = "1")]
    pub heads: Vec<Vec<u8>>,
    /// Peer ID of the sender
    #[prost(string, tag = "2")]
    pub peer_id: String,
    /// Timestamp of the announcement
    #[prost(int64, tag = "3")]
    pub timestamp: i64,
}

/// Fetch operations request
#[derive(Clone, PartialEq, Message)]
pub struct SavedFetchOpsReq {
    /// Operation hashes to fetch
    #[prost(bytes, repeated, tag = "1")]
    pub ops: Vec<Vec<u8>>,
    /// Maximum number of operations to return
    #[prost(uint32, tag = "2")]
    pub max_ops: u32,
}

/// Fetch operations response
#[derive(Clone, PartialEq, Message)]
pub struct SavedFetchOpsResp {
    /// Requested operations
    #[prost(message, repeated, tag = "1")]
    pub ops: Vec<OpHeader>,
}

/// Have chunks request
#[derive(Clone, PartialEq, Message)]
pub struct SavedHaveChunksReq {
    /// Chunk CIDs to check availability for
    #[prost(bytes, repeated, tag = "1")]
    pub chunks: Vec<Vec<u8>>,
}

/// Have chunks response
#[derive(Clone, PartialEq, Message)]
pub struct SavedHaveChunksResp {
    /// Availability bitmap (true = have, false = don't have)
    #[prost(bool, repeated, tag = "1")]
    pub available: Vec<bool>,
}

/// Fetch chunks request
#[derive(Clone, PartialEq, Message)]
pub struct SavedFetchChunksReq {
    /// Chunk CIDs to fetch
    #[prost(bytes, repeated, tag = "1")]
    pub chunks: Vec<Vec<u8>>,
}

/// Fetch chunks response
#[derive(Clone, PartialEq, Message)]
pub struct SavedFetchChunksResp {
    /// Chunk data (encrypted)
    #[prost(bytes, repeated, tag = "1")]
    pub chunks: Vec<Vec<u8>>,
}

/// Acknowledgment message
#[derive(Clone, PartialEq, Message)]
pub struct SavedAck {
    /// Operations acknowledged up to
    #[prost(bytes, repeated, tag = "1")]
    pub up_to_op: Vec<Vec<u8>>,
    /// Timestamp of acknowledgment
    #[prost(uint64, tag = "2")]
    pub timestamp: u64,
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
            timestamp: 1234567890,
            nonce: vec![17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40],
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

    #[test]
    fn test_edit_message_serialization() {
        let body = EditMessageBody {
            msg_id: vec![1, 2, 3, 4],
            payload: Some(edit_message_body::Payload::BodyFull(
                "Updated message".to_string(),
            )),
            edited_at_ms: 1234567890,
        };

        let encoded = body.encode_to_vec();
        let decoded = EditMessageBody::decode(&encoded[..]).unwrap();

        assert_eq!(body, decoded);
    }

    #[test]
    fn test_edit_message_delta_serialization() {
        let body = EditMessageBody {
            msg_id: vec![1, 2, 3, 4],
            payload: Some(edit_message_body::Payload::BodyDelta(vec![1, 2, 3, 4, 5])),
            edited_at_ms: 1234567890,
        };

        let encoded = body.encode_to_vec();
        let decoded = EditMessageBody::decode(&encoded[..]).unwrap();

        assert_eq!(body, decoded);
    }

    #[test]
    fn test_delete_message_serialization() {
        let body = DeleteMessageBody {
            msg_id: vec![1, 2, 3, 4],
            reason: "User requested deletion".to_string(),
            deleted_at_ms: 1234567890,
        };

        let encoded = body.encode_to_vec();
        let decoded = DeleteMessageBody::decode(&encoded[..]).unwrap();

        assert_eq!(body, decoded);
    }

    #[test]
    fn test_attach_body_serialization() {
        let body = AttachBody {
            msg_id: vec![1, 2, 3, 4],
            att_cids: vec![vec![5, 6, 7, 8], vec![9, 10, 11, 12]],
        };

        let encoded = body.encode_to_vec();
        let decoded = AttachBody::decode(&encoded[..]).unwrap();

        assert_eq!(body, decoded);
    }

    #[test]
    fn test_detach_body_serialization() {
        let body = DetachBody {
            msg_id: vec![1, 2, 3, 4],
            att_cids: vec![vec![5, 6, 7, 8]],
        };

        let encoded = body.encode_to_vec();
        let decoded = DetachBody::decode(&encoded[..]).unwrap();

        assert_eq!(body, decoded);
    }

    #[test]
    fn test_ack_body_serialization() {
        let body = AckBody {
            up_to_op: vec![1, 2, 3, 4],
            ts: 1234567890,
        };

        let encoded = body.encode_to_vec();
        let decoded = AckBody::decode(&encoded[..]).unwrap();

        assert_eq!(body, decoded);
    }

    #[test]
    fn test_purge_body_serialization() {
        let body = PurgeBody {
            msg_id: vec![1, 2, 3, 4],
            ts: 1234567890,
        };

        let encoded = body.encode_to_vec();
        let decoded = PurgeBody::decode(&encoded[..]).unwrap();

        assert_eq!(body, decoded);
    }

    #[test]
    fn test_op_envelope_serialization() {
        let header = OpHeader {
            op_id: vec![1, 2, 3, 4],
            lamport: 42,
            parents: vec![vec![5, 6, 7, 8]],
            signer: vec![9, 10, 11, 12],
            sig: vec![13, 14, 15, 16],
            timestamp: 1234567890,
            nonce: vec![17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40],
        };

        let envelope = OpEnvelope {
            header: Some(header),
            ciphertext: vec![17, 18, 19, 20],
        };

        let encoded = envelope.encode_to_vec();
        let decoded = OpEnvelope::decode(&encoded[..]).unwrap();

        assert_eq!(envelope, decoded);
    }

    #[test]
    fn test_announce_heads_serialization() {
        let announce = AnnounceHeads {
            feed_id: "default".to_string(),
            lamport: 42,
            heads: vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8]],
        };

        let encoded = announce.encode_to_vec();
        let decoded = AnnounceHeads::decode(&encoded[..]).unwrap();

        assert_eq!(announce, decoded);
    }

    #[test]
    fn test_fetch_ops_req_serialization() {
        let req = FetchOpsReq {
            since_heads: vec![vec![1, 2, 3, 4]],
            want_max: 100,
        };

        let encoded = req.encode_to_vec();
        let decoded = FetchOpsReq::decode(&encoded[..]).unwrap();

        assert_eq!(req, decoded);
    }

    #[test]
    fn test_fetch_ops_resp_serialization() {
        let header = OpHeader {
            op_id: vec![1, 2, 3, 4],
            lamport: 42,
            parents: vec![vec![5, 6, 7, 8]],
            signer: vec![9, 10, 11, 12],
            sig: vec![13, 14, 15, 16],
            timestamp: 1234567890,
            nonce: vec![17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40],
        };

        let envelope = OpEnvelope {
            header: Some(header),
            ciphertext: vec![17, 18, 19, 20],
        };

        let resp = FetchOpsResp {
            ops: vec![envelope],
            new_heads: vec![vec![21, 22, 23, 24]],
        };

        let encoded = resp.encode_to_vec();
        let decoded = FetchOpsResp::decode(&encoded[..]).unwrap();

        assert_eq!(resp, decoded);
    }

    #[test]
    fn test_have_chunks_req_serialization() {
        let req = HaveChunksReq {
            cids: vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8]],
        };

        let encoded = req.encode_to_vec();
        let decoded = HaveChunksReq::decode(&encoded[..]).unwrap();

        assert_eq!(req, decoded);
    }

    #[test]
    fn test_have_chunks_resp_serialization() {
        let resp = HaveChunksResp {
            have_bitmap: vec![0b10101010, 0b01010101],
        };

        let encoded = resp.encode_to_vec();
        let decoded = HaveChunksResp::decode(&encoded[..]).unwrap();

        assert_eq!(resp, decoded);
    }

    #[test]
    fn test_fetch_chunks_req_serialization() {
        let req = FetchChunksReq {
            cids: vec![vec![1, 2, 3, 4]],
        };

        let encoded = req.encode_to_vec();
        let decoded = FetchChunksReq::decode(&encoded[..]).unwrap();

        assert_eq!(req, decoded);
    }

    #[test]
    fn test_fetch_chunks_resp_serialization() {
        let chunk = fetch_chunks_resp::Chunk {
            cid: vec![1, 2, 3, 4],
            data: vec![5, 6, 7, 8],
        };

        let resp = FetchChunksResp {
            chunks: vec![chunk],
        };

        let encoded = resp.encode_to_vec();
        let decoded = FetchChunksResp::decode(&encoded[..]).unwrap();

        assert_eq!(resp, decoded);
    }

    #[test]
    fn test_onboarding_token_serialization() {
        let token = OnboardingToken {
            account_pubkey: vec![1, 2, 3, 4],
            expires_at: 1234567890,
            nonce: vec![5, 6, 7, 8],
            signature: vec![9, 10, 11, 12],
        };

        let encoded = token.encode_to_vec();
        let decoded = OnboardingToken::decode(&encoded[..]).unwrap();

        assert_eq!(token, decoded);
    }

    #[test]
    fn test_protocol_constants() {
        assert_eq!(protocol::HEADS_TOPIC, "/savedmsgs/{account_id}/heads");
        assert_eq!(protocol::OPS_PROTOCOL, "/savedmsgs/1/ops");
        assert_eq!(protocol::CHUNKS_PROTOCOL, "/savedmsgs/1/chunks");
        assert_eq!(protocol::LINKING_PROTOCOL, "/savedmsgs/1/linking");
    }

    #[test]
    fn test_empty_fields_serialization() {
        // Test that empty fields serialize/deserialize correctly
        let header = OpHeader {
            op_id: vec![],
            lamport: 0,
            parents: vec![],
            signer: vec![],
            sig: vec![],
            timestamp: 0,
            nonce: vec![],
        };

        let encoded = header.encode_to_vec();
        let decoded = OpHeader::decode(&encoded[..]).unwrap();

        assert_eq!(header, decoded);
    }

    #[test]
    fn test_large_data_serialization() {
        // Test with larger data to ensure protobuf handles it correctly
        let large_data = vec![42u8; 10000];
        let body = CreateMessageBody {
            msg_id: large_data.clone(),
            feed_id: "test".to_string(),
            body: "Large message body".to_string(),
            att_cids: vec![large_data.clone(), large_data],
            created_at_ms: 1234567890,
        };

        let encoded = body.encode_to_vec();
        let decoded = CreateMessageBody::decode(&encoded[..]).unwrap();

        assert_eq!(body, decoded);
    }
}
