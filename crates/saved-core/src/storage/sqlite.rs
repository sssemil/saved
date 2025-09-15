//! SQLite storage implementation for SAVED
//!
//! Implements:
//! - SQLite database for metadata and indices (WAL mode)
//! - Content-addressed chunk store for encrypted file attachments
//! - Reference counting for garbage collection

use crate::error::{Error, Result};
use crate::error_recovery::ErrorRecoveryManager;
use crate::events::Op;
use crate::protobuf::OpEnvelope;
use crate::types::{Message, MessageId};
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

use super::trait_impl::{Storage, StorageStats};
use super::trait_impl::Attachment;

/// Chunk identifier (BLAKE3 hash of plaintext)
pub type ChunkId = [u8; 32];

/// SQLite storage implementation
pub struct SqliteStorage {
    /// SQLite connection (thread-safe)
    db: Arc<Mutex<Connection>>,
    /// Path to the chunks directory
    chunks_path: PathBuf,
    /// Reference counts for chunks
    chunk_refs: Mutex<HashMap<ChunkId, u32>>,
    /// Error recovery manager
    error_recovery: Arc<Mutex<ErrorRecoveryManager>>,
}

impl SqliteStorage {
    /// Open or create SQLite storage at the given path
    pub fn open(account_path: PathBuf) -> Result<Self> {
        // Ensure account directory exists
        fs::create_dir_all(&account_path)?;

        let chunks_path = account_path.join("chunks");
        fs::create_dir_all(&chunks_path)?;

        // Open SQLite database
        let db_path = account_path.join("db.sqlite");
        let db = Connection::open(db_path)?;

        // Enable WAL mode and other performance pragmas
        db.pragma_update(None, "journal_mode", &"WAL")?;
        db.pragma_update(None, "synchronous", &"NORMAL")?;
        db.pragma_update(None, "cache_size", &10000i64)?;
        db.pragma_update(None, "temp_store", &"MEMORY")?;

        let mut storage = Self {
            db: Arc::new(Mutex::new(db)),
            chunks_path,
            chunk_refs: Mutex::new(HashMap::new()),
            error_recovery: Arc::new(Mutex::new(ErrorRecoveryManager::new())),
        };

        storage.init_schema()?;
        storage.load_chunk_refs()?;

        Ok(storage)
    }

    /// Initialize database schema
    fn init_schema(&mut self) -> Result<()> {
        let db = self.db.lock().unwrap();
        // Operations table
        db.execute(
            "CREATE TABLE IF NOT EXISTS operations (
                hash BLOB PRIMARY KEY,
                op_id BLOB NOT NULL,
                device_pubkey BLOB NOT NULL,
                counter INTEGER NOT NULL,
                lamport INTEGER NOT NULL,
                parents BLOB NOT NULL,
                operation_data BLOB NOT NULL,
                timestamp INTEGER NOT NULL
            )",
            [],
        )?;

        // Encrypted operations table
        db.execute(
            "CREATE TABLE IF NOT EXISTS encrypted_operations (
                op_id BLOB PRIMARY KEY,
                header_data BLOB NOT NULL,
                ciphertext BLOB NOT NULL
            )",
            [],
        )?;

        // Messages table (materialized view)
        db.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id BLOB PRIMARY KEY,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                is_deleted BOOLEAN NOT NULL DEFAULT 0,
                is_purged BOOLEAN NOT NULL DEFAULT 0
            )",
            [],
        )?;

        // Chunk references table
        db.execute(
            "CREATE TABLE IF NOT EXISTS chunk_refs (
                chunk_id BLOB PRIMARY KEY,
                ref_count INTEGER NOT NULL DEFAULT 1
            )",
            [],
        )?;

        // Attachments metadata
        db.execute(
            "CREATE TABLE IF NOT EXISTS attachments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id BLOB NOT NULL,
                filename TEXT NOT NULL,
                size INTEGER NOT NULL,
                file_hash BLOB NOT NULL,
                mime_type TEXT,
                status INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_accessed INTEGER
            )",
            [],
        )?;

        // Attachment chunks mapping
        db.execute(
            "CREATE TABLE IF NOT EXISTS attachment_chunks (
                attachment_id INTEGER NOT NULL,
                chunk_id BLOB NOT NULL,
                ord INTEGER NOT NULL,
                PRIMARY KEY (attachment_id, ord)
            )",
            [],
        )?;

        // Account keys table (encrypted with passphrase)
        db.execute(
            "CREATE TABLE IF NOT EXISTS account_keys (
                id INTEGER PRIMARY KEY,
                encrypted_key BLOB NOT NULL
            )",
            [],
        )?;

        // Account key info table (public metadata)
        db.execute(
            "CREATE TABLE IF NOT EXISTS account_key_info (
                id INTEGER PRIMARY KEY,
                public_key BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER,
                version INTEGER NOT NULL,
                has_private_key INTEGER NOT NULL
            )",
            [],
        )?;

        // Shared account keys table (for device-to-device sharing)
        db.execute(
            "CREATE TABLE IF NOT EXISTS shared_account_keys (
                id INTEGER PRIMARY KEY,
                encrypted_key BLOB NOT NULL,
                shared_at INTEGER NOT NULL
            )",
            [],
        )?;

        // Vault keys table (encrypted with passphrase)
        db.execute(
            "CREATE TABLE IF NOT EXISTS vault_keys (
                id INTEGER PRIMARY KEY,
                encrypted_key BLOB NOT NULL
            )",
            [],
        )?;

        // Authorized devices table
        db.execute(
            "CREATE TABLE IF NOT EXISTS authorized_devices (
                device_id TEXT PRIMARY KEY,
                device_cert BLOB NOT NULL,
                authorized_at INTEGER NOT NULL
            )",
            [],
        )?;

        // Device certificate table (for this device)
        db.execute(
            "CREATE TABLE IF NOT EXISTS device_certificate (
                id INTEGER PRIMARY KEY,
                device_cert BLOB NOT NULL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        // Device info table (stores device key material)
        db.execute(
            "CREATE TABLE IF NOT EXISTS device_info (
                id INTEGER PRIMARY KEY,
                device_key BLOB NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    /// Load chunk reference counts from database
    fn load_chunk_refs(&mut self) -> Result<()> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT chunk_id, ref_count FROM chunk_refs")?;
        let rows = stmt.query_map([], |row| {
            let chunk_id: Vec<u8> = row.get(0)?;
            let ref_count: u32 = row.get(1)?;
            Ok((chunk_id, ref_count))
        })?;

        let mut map = self.chunk_refs.lock().unwrap();
        for row in rows {
            let (chunk_id_bytes, ref_count) = row?;
            if chunk_id_bytes.len() == 32 {
                let mut chunk_id = [0u8; 32];
                chunk_id.copy_from_slice(&chunk_id_bytes);
                map.insert(chunk_id, ref_count);
            }
        }

        Ok(())
    }

}

#[async_trait]
impl Storage for SqliteStorage {
    async fn init(&mut self) -> Result<()> {
        // Schema is already initialized in constructor
        Ok(())
    }

    async fn store_operation(&self, operation: &Op) -> Result<()> {
        let hash = operation.hash();
        let db = self.db.lock().unwrap();

        db.execute(
            "INSERT OR REPLACE INTO operations (
                hash, op_id, device_pubkey, counter, lamport, parents,
                operation_data, timestamp
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                hash.as_slice(),
                operation.id.to_bytes(),
                operation.id.device_pubkey.as_slice(),
                operation.id.counter,
                operation.lamport,
                bincode::serialize(&operation.parents).map_err(|e| crate::error::Error::Sync(
                    format!("Serialization error: {}", e)
                ))?,
                bincode::serialize(&operation.operation).map_err(|e| crate::error::Error::Sync(
                    format!("Operation serialization error: {}", e)
                ))?,
                operation.timestamp.timestamp()
            ],
        )?;

        Ok(())
    }

    async fn store_encrypted_operation(&self, envelope: &OpEnvelope) -> Result<()> {
        let db = self.db.lock().unwrap();
        
        // Extract operation ID from header for indexing
        let op_id = if let Some(header) = &envelope.header {
            header.op_id.clone()
        } else {
            return Err(crate::error::Error::Sync("Missing operation header".to_string()));
        };

        db.execute(
            "INSERT OR REPLACE INTO encrypted_operations (
                op_id, header_data, ciphertext
            ) VALUES (?, ?, ?)",
            params![
                op_id,
                bincode::serialize(&envelope.header).map_err(|e| crate::error::Error::Sync(
                    format!("Header serialization error: {}", e)
                ))?,
                envelope.ciphertext,
            ],
        )?;
        Ok(())
    }

    async fn get_all_operations(&self) -> Result<Vec<Op>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT hash, op_id, device_pubkey, counter, lamport, parents, operation_data, timestamp FROM operations ORDER BY timestamp"
        )?;

        let rows = stmt.query_map([], |row| {
            let _hash_bytes: Vec<u8> = row.get(0)?;
            let op_id_bytes: Vec<u8> = row.get(1)?;
            let device_pubkey_bytes: Vec<u8> = row.get(2)?;
            let _counter: u64 = row.get(3)?;
            let lamport: u64 = row.get(4)?;
            let parents_bytes: Vec<u8> = row.get(5)?;
            let operation_data: Vec<u8> = row.get(6)?;
            let timestamp: i64 = row.get(7)?;

            // Convert bytes back to proper types
            let mut device_pubkey = [0u8; 32];
            device_pubkey.copy_from_slice(&device_pubkey_bytes[..32]);

            let op_id = crate::events::OpId::from_bytes(&op_id_bytes).map_err(|e| {
                rusqlite::Error::InvalidParameterName(format!("OpId deserialization error: {}", e))
            })?;
            let parents: Vec<crate::events::OpHash> = bincode::deserialize(&parents_bytes)
                .map_err(|e| {
                    rusqlite::Error::InvalidParameterName(format!("Deserialization error: {}", e))
                })?;
            let operation: crate::events::Operation = bincode::deserialize(&operation_data)
                .map_err(|e| {
                    rusqlite::Error::InvalidParameterName(format!(
                        "Operation deserialization error: {}",
                        e
                    ))
                })?;

            Ok(Op {
                id: op_id,
                lamport,
                parents,
                operation,
                timestamp: chrono::DateTime::from_timestamp(timestamp, 0)
                    .unwrap_or_default()
                    .with_timezone(&chrono::Utc),
            })
        })?;

        let mut operations = Vec::new();
        for row in rows {
            operations.push(row?);
        }

        Ok(operations)
    }

    async fn get_all_encrypted_operations(&self) -> Result<Vec<OpEnvelope>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT header_data, ciphertext FROM encrypted_operations ORDER BY op_id"
        )?;

        let rows = stmt.query_map([], |row| {
            let header_data: Vec<u8> = row.get(0)?;
            let ciphertext: Vec<u8> = row.get(1)?;

            let header: Option<crate::protobuf::OpHeader> = bincode::deserialize(&header_data)
                .map_err(|e| rusqlite::Error::InvalidColumnType(
                    0,
                    format!("Header deserialization error: {}", e),
                    rusqlite::types::Type::Blob,
                ))?;

            Ok(OpEnvelope {
                header,
                ciphertext,
            })
        })?;

        let mut operations = Vec::new();
        for row in rows {
            operations.push(row?);
        }

        Ok(operations)
    }

    async fn get_device_operations(&self, device_id: &[u8; 32]) -> Result<Vec<Op>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT hash, op_id, device_pubkey, counter, lamport, parents, operation_data, timestamp FROM operations WHERE device_pubkey = ? ORDER BY timestamp"
        )?;

        let rows = stmt.query_map(params![device_id.as_slice()], |row| {
            let _hash_bytes: Vec<u8> = row.get(0)?;
            let op_id_bytes: Vec<u8> = row.get(1)?;
            let device_pubkey_bytes: Vec<u8> = row.get(2)?;
            let _counter: u64 = row.get(3)?;
            let lamport: u64 = row.get(4)?;
            let parents_bytes: Vec<u8> = row.get(5)?;
            let operation_data: Vec<u8> = row.get(6)?;
            let timestamp: i64 = row.get(7)?;

            // Convert bytes back to proper types
            let mut device_pubkey = [0u8; 32];
            device_pubkey.copy_from_slice(&device_pubkey_bytes[..32]);

            let op_id = crate::events::OpId::from_bytes(&op_id_bytes).map_err(|e| {
                rusqlite::Error::InvalidParameterName(format!("OpId deserialization error: {}", e))
            })?;
            let parents: Vec<crate::events::OpHash> = bincode::deserialize(&parents_bytes)
                .map_err(|e| {
                    rusqlite::Error::InvalidParameterName(format!("Deserialization error: {}", e))
                })?;
            let operation: crate::events::Operation = bincode::deserialize(&operation_data)
                .map_err(|e| {
                    rusqlite::Error::InvalidParameterName(format!(
                        "Operation deserialization error: {}",
                        e
                    ))
                })?;

            Ok(Op {
                id: op_id,
                lamport,
                parents,
                operation,
                timestamp: chrono::DateTime::from_timestamp(timestamp, 0)
                    .unwrap_or_default()
                    .with_timezone(&chrono::Utc),
            })
        })?;

        let mut operations = Vec::new();
        for row in rows {
            operations.push(row?);
        }

        Ok(operations)
    }

    async fn store_chunk(&self, hash: &[u8; 32], data: &[u8]) -> Result<()> {
        let chunk_path = self.chunks_path.join(hex::encode(hash));
        fs::write(&chunk_path, data)?;

        // Update in-memory ref count map
        {
            let mut map = self.chunk_refs.lock().unwrap();
            let new_count = map.get(hash).copied().unwrap_or(0).saturating_add(1);
            map.insert(*hash, new_count);
        }

        // Persist ref count to database
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO chunk_refs (chunk_id, ref_count) VALUES (?, 1)
             ON CONFLICT(chunk_id) DO UPDATE SET ref_count = ref_count + 1",
            params![hash.as_slice()],
        )?;
        Ok(())
    }

    async fn get_chunk(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let chunk_path = self.chunks_path.join(hex::encode(hash));
        if chunk_path.exists() {
            Ok(Some(fs::read(&chunk_path)?))
        } else {
            Ok(None)
        }
    }

    async fn has_chunk(&self, hash: &[u8; 32]) -> Result<bool> {
        let chunk_path = self.chunks_path.join(hex::encode(hash));
        Ok(chunk_path.exists())
    }

    async fn get_all_messages(&self) -> Result<Vec<Message>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, content, created_at, is_deleted, is_purged FROM messages WHERE is_deleted = 0 ORDER BY created_at"
        )?;

        let rows = stmt.query_map([], |row| {
            let id_bytes: Vec<u8> = row.get(0)?;
            let content: String = row.get(1)?;
            let created_at: i64 = row.get(2)?;
            let is_deleted: bool = row.get(3)?;
            let is_purged: bool = row.get(4)?;

            let mut id = [0u8; 32];
            id.copy_from_slice(&id_bytes[..32]);

            Ok(Message {
                id: MessageId(id),
                content,
                created_at: chrono::DateTime::from_timestamp(created_at, 0)
                    .unwrap_or_default()
                    .with_timezone(&chrono::Utc),
                is_deleted,
                is_purged,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }

        Ok(messages)
    }

    async fn get_all_messages_including_deleted(&self) -> Result<Vec<Message>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, content, created_at, is_deleted, is_purged FROM messages ORDER BY created_at"
        )?;

        let rows = stmt.query_map([], |row| {
            let id_bytes: Vec<u8> = row.get(0)?;
            let content: String = row.get(1)?;
            let created_at: i64 = row.get(2)?;
            let is_deleted: bool = row.get(3)?;
            let is_purged: bool = row.get(4)?;

            let mut id = [0u8; 32];
            id.copy_from_slice(&id_bytes[..32]);

            Ok(Message {
                id: MessageId(id),
                content,
                created_at: chrono::DateTime::from_timestamp(created_at, 0)
                    .unwrap_or_default()
                    .with_timezone(&chrono::Utc),
                is_deleted,
                is_purged,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }

        Ok(messages)
    }

    async fn get_message(&self, message_id: &MessageId) -> Result<Option<Message>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, content, created_at, is_deleted, is_purged FROM messages WHERE id = ?",
        )?;

        let mut rows = stmt.query_map(params![message_id.0.as_slice()], |row| {
            let id_bytes: Vec<u8> = row.get(0)?;
            let content: String = row.get(1)?;
            let created_at: i64 = row.get(2)?;
            let is_deleted: bool = row.get(3)?;
            let is_purged: bool = row.get(4)?;

            let mut id = [0u8; 32];
            id.copy_from_slice(&id_bytes[..32]);

            Ok(Message {
                id: MessageId(id),
                content,
                created_at: chrono::DateTime::from_timestamp(created_at, 0)
                    .unwrap_or_default()
                    .with_timezone(&chrono::Utc),
                is_deleted,
                is_purged,
            })
        })?;

        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    async fn store_message(&self, message: &Message) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO messages (id, content, created_at, is_deleted, is_purged) VALUES (?, ?, ?, ?, ?)",
            params![
                message.id.0.as_slice(),
                message.content,
                message.created_at.timestamp(),
                message.is_deleted,
                message.is_purged
            ],
        )?;
        Ok(())
    }

    async fn delete_message(&self, message_id: &MessageId) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM messages WHERE id = ?",
            params![message_id.0.as_slice()],
        )?;
        Ok(())
    }

    async fn store_account_key(&self, encrypted_account_key: &[u8]) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO account_keys (id, encrypted_key) VALUES (1, ?)",
            params![encrypted_account_key],
        )?;
        Ok(())
    }

    async fn get_account_key(&self) -> Result<Option<Vec<u8>>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT encrypted_key FROM account_keys WHERE id = 1")?;
        let result = stmt.query_row([], |row| {
            let encrypted_key: Vec<u8> = row.get(0)?;
            Ok(encrypted_key)
        });
        
        match result {
            Ok(key) => Ok(Some(key)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn store_account_key_info(&self, key_info: &crate::crypto::AccountKeyInfo) -> Result<()> {
        let db = self.db.lock().unwrap();
        let created_at = key_info.created_at.timestamp();
        let expires_at = key_info.expires_at.map(|t| t.timestamp());
        let has_private_key = if key_info.has_private_key { 1 } else { 0 };
        
        db.execute(
            "INSERT OR REPLACE INTO account_key_info (id, public_key, created_at, expires_at, version, has_private_key) VALUES (1, ?, ?, ?, ?, ?)",
            params![key_info.public_key, created_at, expires_at, key_info.version, has_private_key],
        )?;
        Ok(())
    }

    async fn get_account_key_info(&self) -> Result<Option<crate::crypto::AccountKeyInfo>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT public_key, created_at, expires_at, version, has_private_key FROM account_key_info WHERE id = 1")?;
        let result = stmt.query_row([], |row| {
            let public_key: Vec<u8> = row.get(0)?;
            let created_at: i64 = row.get(1)?;
            let expires_at: Option<i64> = row.get(2)?;
            let version: u64 = row.get(3)?;
            let has_private_key: i64 = row.get(4)?;
            
            let mut pub_key_bytes = [0u8; 32];
            pub_key_bytes.copy_from_slice(&public_key[..32]);
            
            Ok(crate::crypto::AccountKeyInfo {
                public_key: pub_key_bytes,
                created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default(),
                expires_at: expires_at.map(|t| chrono::DateTime::from_timestamp(t, 0).unwrap_or_default()),
                version,
                has_private_key: has_private_key != 0,
            })
        });
        
        match result {
            Ok(info) => Ok(Some(info)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn store_shared_account_key(&self, encrypted_account_key: &[u8]) -> Result<()> {
        let db = self.db.lock().unwrap();
        let shared_at = chrono::Utc::now().timestamp();
        db.execute(
            "INSERT OR REPLACE INTO shared_account_keys (id, encrypted_key, shared_at) VALUES (1, ?, ?)",
            params![encrypted_account_key, shared_at],
        )?;
        Ok(())
    }

    async fn get_shared_account_key(&self) -> Result<Option<Vec<u8>>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT encrypted_key FROM shared_account_keys WHERE id = 1")?;
        let result = stmt.query_row([], |row| {
            let encrypted_key: Vec<u8> = row.get(0)?;
            Ok(encrypted_key)
        });
        
        match result {
            Ok(key) => Ok(Some(key)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn store_vault_key(&self, encrypted_vault_key: &[u8]) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO vault_keys (id, encrypted_key) VALUES (1, ?)",
            params![encrypted_vault_key],
        )?;
        Ok(())
    }

    async fn get_vault_key(&self) -> Result<Option<Vec<u8>>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT encrypted_key FROM vault_keys WHERE id = 1")?;
        let result = stmt.query_row([], |row| {
            let encrypted_key: Vec<u8> = row.get(0)?;
            Ok(encrypted_key)
        });

        match result {
            Ok(key) => Ok(Some(key)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn store_authorized_device(&self, device_id: &str, device_cert: &[u8]) -> Result<()> {
        let db = self.db.lock().unwrap();
        let authorized_at = chrono::Utc::now().timestamp();
        db.execute(
            "INSERT OR REPLACE INTO authorized_devices (device_id, device_cert, authorized_at) VALUES (?, ?, ?)",
            params![device_id, device_cert, authorized_at],
        )?;
        Ok(())
    }

    async fn get_authorized_devices(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT device_id, device_cert FROM authorized_devices")?;
        let rows = stmt.query_map([], |row| {
            let device_id: String = row.get(0)?;
            let device_cert: Vec<u8> = row.get(1)?;
            Ok((device_id, device_cert))
        })?;

        let mut devices = Vec::new();
        for row in rows {
            devices.push(row?);
        }
        Ok(devices)
    }

    async fn revoke_device_authorization(&self, device_id: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM authorized_devices WHERE device_id = ?",
            params![device_id],
        )?;
        Ok(())
    }

    async fn is_device_authorized(&self, device_id: &str) -> Result<bool> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT 1 FROM authorized_devices WHERE device_id = ?")?;
        let result = stmt.query_row(params![device_id], |_| Ok(true));
        
        match result {
            Ok(_) => Ok(true),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    async fn store_device_certificate(&self, device_cert: &crate::crypto::DeviceCert) -> Result<()> {
        let db = self.db.lock().unwrap();
        let created_at = chrono::Utc::now().timestamp();
        let cert_bytes = bincode::serialize(device_cert)
            .map_err(|e| crate::error::Error::Crypto(format!("Failed to serialize device cert: {}", e)))?;
        
        db.execute(
            "INSERT OR REPLACE INTO device_certificate (id, device_cert, created_at) VALUES (1, ?, ?)",
            params![cert_bytes, created_at],
        )?;
        Ok(())
    }

    async fn get_device_certificate(&self) -> Result<Option<crate::crypto::DeviceCert>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT device_cert FROM device_certificate WHERE id = 1")?;
        let result = stmt.query_row([], |row| {
            let cert_bytes: Vec<u8> = row.get(0)?;
            let device_cert: crate::crypto::DeviceCert = bincode::deserialize(&cert_bytes)
                .map_err(|_e| rusqlite::Error::InvalidColumnType(0, "device_cert".to_string(), rusqlite::types::Type::Blob))?;
            Ok(device_cert)
        });
        
        match result {
            Ok(cert) => Ok(Some(cert)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_device_key(&self) -> Result<crate::crypto::DeviceKey> {
        // Retrieve the stored device key from SQLite database
        let conn = self.db.lock().unwrap();
        
        let result = conn.query_row(
            "SELECT device_key FROM device_info WHERE id = 1",
            [],
            |row| {
                let key_bytes: Vec<u8> = row.get(0)?;
                Ok(key_bytes)
            },
        );
        
        match result {
            Ok(key_bytes) => {
                // Deserialize the stored device key from raw bytes
                if key_bytes.len() == 32 {
                    // Old format - just signing key bytes
                    let mut signing_key_bytes = [0u8; 32];
                    signing_key_bytes.copy_from_slice(&key_bytes);
                    let device_key = crate::crypto::DeviceKey::from_bytes(&signing_key_bytes)?;
                    Ok(device_key)
                } else if key_bytes.len() == 64 {
                    // New format - both signing and verifying key bytes
                    let mut signing_key_bytes = [0u8; 32];
                    let mut verifying_key_bytes = [0u8; 32];
                    signing_key_bytes.copy_from_slice(&key_bytes[0..32]);
                    verifying_key_bytes.copy_from_slice(&key_bytes[32..64]);
                    
                    let signing_key = ed25519_dalek::SigningKey::from_bytes(&signing_key_bytes);
                    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&verifying_key_bytes)
                        .map_err(|e| Error::Storage(format!("Invalid verifying key: {}", e)))?;
                    
                    Ok(crate::crypto::DeviceKey::from_keys(signing_key, verifying_key))
                } else {
                    return Err(Error::Storage("Invalid device key length".to_string()));
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Generate a new device key if none exists
                let new_device_key = crate::crypto::DeviceKey::generate();
                
                // Store the new device key as raw bytes (64 bytes: 32 signing + 32 verifying)
                let mut key_bytes = Vec::new();
                key_bytes.extend_from_slice(&new_device_key.private_key_bytes());
                key_bytes.extend_from_slice(&new_device_key.public_key_bytes());
                
                conn.execute(
                    "INSERT OR REPLACE INTO device_info (id, device_key) VALUES (1, ?)",
                    [key_bytes],
                ).map_err(|e| Error::Storage(format!("Failed to store device key: {}", e)))?;
                
                Ok(new_device_key)
            }
            Err(e) => Err(Error::Storage(format!("Failed to retrieve device key: {}", e))),
        }
    }

    async fn store_device_key(&self, device_key: &crate::crypto::DeviceKey) -> Result<()> {
        let conn = self.db.lock().unwrap();
        
        // Store the device key as raw bytes (64 bytes: 32 signing + 32 verifying)
        let mut key_bytes = Vec::new();
        key_bytes.extend_from_slice(&device_key.private_key_bytes());
        key_bytes.extend_from_slice(&device_key.public_key_bytes());
        
        conn.execute(
            "INSERT OR REPLACE INTO device_info (id, device_key) VALUES (1, ?)",
            [key_bytes],
        ).map_err(|e| Error::Storage(format!("Failed to store device key: {}", e)))?;
        
        Ok(())
    }

    async fn get_stats(&self) -> Result<StorageStats> {
        let db = self.db.lock().unwrap();
        // Count operations
        let mut stmt = db.prepare("SELECT COUNT(*) FROM operations")?;
        let operation_count: usize = stmt.query_row([], |row| row.get(0))?;

        // Count messages
        let mut stmt = db.prepare("SELECT COUNT(*) FROM messages")?;
        let message_count: usize = stmt.query_row([], |row| row.get(0))?;

        // Count chunks and calculate total size
        let mut chunk_count = 0;
        let mut total_size = 0u64;

        for entry in WalkDir::new(&self.chunks_path) {
            let entry = entry?;
            if entry.file_type().is_file() {
                chunk_count += 1;
                total_size += entry.metadata()?.len();
            }
        }

        Ok(StorageStats {
            operation_count,
            message_count,
            chunk_count,
            total_size,
        })
    }

    async fn store_attachment(&self, message_id: &MessageId, filename: &str, size: u64, file_hash: &[u8; 32], mime_type: Option<String>, chunk_ids: &Vec<[u8; 32]>) -> Result<i64> {
        let db = self.db.lock().unwrap();
        let created_at = chrono::Utc::now().timestamp();
        db.execute(
            "INSERT INTO attachments (message_id, filename, size, file_hash, mime_type, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                message_id.0.as_slice(), 
                filename, 
                size as i64, 
                file_hash.as_slice(),
                mime_type,
                0i64, // Active status
                created_at
            ],
        )?;

        let attachment_id: i64 = db.last_insert_rowid();

        for (idx, cid) in chunk_ids.iter().enumerate() {
            db.execute(
                "INSERT INTO attachment_chunks (attachment_id, chunk_id, ord) VALUES (?, ?, ?)",
                params![attachment_id, cid.as_slice(), idx as i64],
            )?;
        }

        Ok(attachment_id)
    }

    async fn get_attachments_for_message(&self, message_id: &MessageId) -> Result<Vec<Attachment>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT id, message_id, filename, size, file_hash, mime_type, status, created_at, last_accessed FROM attachments WHERE message_id = ? ORDER BY created_at")?;
        let rows = stmt.query_map(params![message_id.0.as_slice()], |row| {
            let id: i64 = row.get(0)?;
            let msg_id_bytes: Vec<u8> = row.get(1)?;
            let filename: String = row.get(2)?;
            let size: i64 = row.get(3)?;
            let file_hash_bytes: Vec<u8> = row.get(4)?;
            let mime_type: Option<String> = row.get(5)?;
            let status: i64 = row.get(6)?;
            let created_at: i64 = row.get(7)?;
            let last_accessed: Option<i64> = row.get(8)?;
            
            let mut id_bytes = [0u8; 32];
            id_bytes.copy_from_slice(&msg_id_bytes[..32]);
            
            let mut file_hash = [0u8; 32];
            file_hash.copy_from_slice(&file_hash_bytes[..32]);
            
            let status = match status {
                0 => super::trait_impl::AttachmentStatus::Active,
                1 => super::trait_impl::AttachmentStatus::Deleted,
                2 => super::trait_impl::AttachmentStatus::Purged,
                _ => super::trait_impl::AttachmentStatus::Active,
            };
            
            Ok(Attachment {
                id,
                message_id: MessageId(id_bytes),
                filename,
                size: size as u64,
                file_hash,
                mime_type,
                status,
                created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default().with_timezone(&chrono::Utc),
                last_accessed: last_accessed.map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&chrono::Utc)),
            })
        })?;

        let mut attachments = Vec::new();
        for row in rows { attachments.push(row?); }
        Ok(attachments)
    }

    async fn get_attachment_chunks(&self, attachment_id: i64) -> Result<Vec<[u8; 32]>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT chunk_id FROM attachment_chunks WHERE attachment_id = ? ORDER BY ord")?;
        let rows = stmt.query_map(params![attachment_id], |row| {
            let cid: Vec<u8> = row.get(0)?;
            let mut id = [0u8; 32];
            id.copy_from_slice(&cid[..32]);
            Ok(id)
        })?;
        let mut chunk_ids = Vec::new();
        for row in rows { chunk_ids.push(row?); }
        Ok(chunk_ids)
    }

    async fn get_all_attachments(&self) -> Result<Vec<Attachment>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT id, message_id, filename, size, file_hash, mime_type, status, created_at, last_accessed FROM attachments ORDER BY created_at")?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let msg_id_bytes: Vec<u8> = row.get(1)?;
            let filename: String = row.get(2)?;
            let size: i64 = row.get(3)?;
            let file_hash_bytes: Vec<u8> = row.get(4)?;
            let mime_type: Option<String> = row.get(5)?;
            let status: i64 = row.get(6)?;
            let created_at: i64 = row.get(7)?;
            let last_accessed: Option<i64> = row.get(8)?;
            
            let mut id_bytes = [0u8; 32];
            id_bytes.copy_from_slice(&msg_id_bytes[..32]);
            
            let mut file_hash = [0u8; 32];
            file_hash.copy_from_slice(&file_hash_bytes[..32]);
            
            let status = match status {
                0 => super::trait_impl::AttachmentStatus::Active,
                1 => super::trait_impl::AttachmentStatus::Deleted,
                2 => super::trait_impl::AttachmentStatus::Purged,
                _ => super::trait_impl::AttachmentStatus::Active,
            };
            
            Ok(Attachment {
                id,
                message_id: MessageId(id_bytes),
                filename,
                size: size as u64,
                file_hash,
                mime_type,
                status,
                created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default().with_timezone(&chrono::Utc),
                last_accessed: last_accessed.map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&chrono::Utc)),
            })
        })?;

        let mut attachments = Vec::new();
        for row in rows { attachments.push(row?); }
        Ok(attachments)
    }

    async fn get_attachment_by_id(&self, attachment_id: i64) -> Result<Option<Attachment>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT id, message_id, filename, size, file_hash, mime_type, status, created_at, last_accessed FROM attachments WHERE id = ?")?;
        let mut rows = stmt.query_map(params![attachment_id], |row| {
            let id: i64 = row.get(0)?;
            let msg_id_bytes: Vec<u8> = row.get(1)?;
            let filename: String = row.get(2)?;
            let size: i64 = row.get(3)?;
            let file_hash_bytes: Vec<u8> = row.get(4)?;
            let mime_type: Option<String> = row.get(5)?;
            let status: i64 = row.get(6)?;
            let created_at: i64 = row.get(7)?;
            let last_accessed: Option<i64> = row.get(8)?;
            
            let mut id_bytes = [0u8; 32];
            id_bytes.copy_from_slice(&msg_id_bytes[..32]);
            
            let mut file_hash = [0u8; 32];
            file_hash.copy_from_slice(&file_hash_bytes[..32]);
            
            let status = match status {
                0 => super::trait_impl::AttachmentStatus::Active,
                1 => super::trait_impl::AttachmentStatus::Deleted,
                2 => super::trait_impl::AttachmentStatus::Purged,
                _ => super::trait_impl::AttachmentStatus::Active,
            };
            
            Ok(Attachment {
                id,
                message_id: MessageId(id_bytes),
                filename,
                size: size as u64,
                file_hash,
                mime_type,
                status,
                created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default().with_timezone(&chrono::Utc),
                last_accessed: last_accessed.map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&chrono::Utc)),
            })
        })?;

        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    async fn get_attachments_by_file_hash(&self, file_hash: &[u8; 32]) -> Result<Vec<Attachment>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT id, message_id, filename, size, file_hash, mime_type, status, created_at, last_accessed FROM attachments WHERE file_hash = ? ORDER BY created_at")?;
        let rows = stmt.query_map(params![file_hash.as_slice()], |row| {
            let id: i64 = row.get(0)?;
            let msg_id_bytes: Vec<u8> = row.get(1)?;
            let filename: String = row.get(2)?;
            let size: i64 = row.get(3)?;
            let file_hash_bytes: Vec<u8> = row.get(4)?;
            let mime_type: Option<String> = row.get(5)?;
            let status: i64 = row.get(6)?;
            let created_at: i64 = row.get(7)?;
            let last_accessed: Option<i64> = row.get(8)?;
            
            let mut id_bytes = [0u8; 32];
            id_bytes.copy_from_slice(&msg_id_bytes[..32]);
            
            let mut file_hash = [0u8; 32];
            file_hash.copy_from_slice(&file_hash_bytes[..32]);
            
            let status = match status {
                0 => super::trait_impl::AttachmentStatus::Active,
                1 => super::trait_impl::AttachmentStatus::Deleted,
                2 => super::trait_impl::AttachmentStatus::Purged,
                _ => super::trait_impl::AttachmentStatus::Active,
            };
            
            Ok(Attachment {
                id,
                message_id: MessageId(id_bytes),
                filename,
                size: size as u64,
                file_hash,
                mime_type,
                status,
                created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default().with_timezone(&chrono::Utc),
                last_accessed: last_accessed.map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&chrono::Utc)),
            })
        })?;

        let mut attachments = Vec::new();
        for row in rows { attachments.push(row?); }
        Ok(attachments)
    }

    async fn delete_attachment(&self, attachment_id: i64) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE attachments SET status = 1 WHERE id = ?",
            params![attachment_id],
        )?;
        Ok(())
    }

    async fn purge_attachment(&self, attachment_id: i64) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE attachments SET status = 2 WHERE id = ?",
            params![attachment_id],
        )?;
        // Remove chunk mappings
        db.execute(
            "DELETE FROM attachment_chunks WHERE attachment_id = ?",
            params![attachment_id],
        )?;
        Ok(())
    }

    async fn update_attachment_access_time(&self, attachment_id: i64) -> Result<()> {
        let db = self.db.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        db.execute(
            "UPDATE attachments SET last_accessed = ? WHERE id = ?",
            params![now, attachment_id],
        )?;
        Ok(())
    }

    async fn get_attachment_stats(&self) -> Result<super::trait_impl::AttachmentStats> {
        let db = self.db.lock().unwrap();
        
        // Count total attachments
        let mut stmt = db.prepare("SELECT COUNT(*) FROM attachments")?;
        let total_attachments: usize = stmt.query_row([], |row| row.get(0))?;
        
        // Count by status
        let mut stmt = db.prepare("SELECT status, COUNT(*) FROM attachments GROUP BY status")?;
        let rows = stmt.query_map([], |row| {
            let status: i64 = row.get(0)?;
            let count: usize = row.get(1)?;
            Ok((status, count))
        })?;
        
        let mut active_attachments = 0;
        let mut deleted_attachments = 0;
        let mut purged_attachments = 0;
        
        for row in rows {
            let (status, count) = row?;
            match status {
                0 => active_attachments = count,
                1 => deleted_attachments = count,
                2 => purged_attachments = count,
                _ => {}
            }
        }
        
        // Calculate total size
        let mut stmt = db.prepare("SELECT SUM(size) FROM attachments")?;
        let total_size: i64 = stmt.query_row([], |row| row.get(0)).unwrap_or(0);
        
        // Count unique files
        let mut stmt = db.prepare("SELECT COUNT(DISTINCT file_hash) FROM attachments")?;
        let unique_files: usize = stmt.query_row([], |row| row.get(0))?;
        
        Ok(super::trait_impl::AttachmentStats {
            total_attachments,
            active_attachments,
            deleted_attachments,
            purged_attachments,
            total_size: total_size as u64,
            unique_files,
        })
    }

    async fn garbage_collect_attachments(&self) -> Result<super::trait_impl::GarbageCollectionStats> {
        let db = self.db.lock().unwrap();
        
        // Get purged attachments and their chunks
        let mut stmt = db.prepare("SELECT a.id, a.size, ac.chunk_id FROM attachments a LEFT JOIN attachment_chunks ac ON a.id = ac.attachment_id WHERE a.status = 2")?;
        let rows = stmt.query_map([], |row| {
            let attachment_id: i64 = row.get(0)?;
            let size: i64 = row.get(1)?;
            let chunk_id: Option<Vec<u8>> = row.get(2)?;
            Ok((attachment_id, size, chunk_id))
        })?;
        
        let mut chunks_to_remove = Vec::new();
        let mut attachments_removed = 0;
        let mut space_freed = 0u64;
        
        for row in rows {
            let (_attachment_id, size, chunk_id) = row?;
            attachments_removed += 1;
            space_freed += size as u64;
            
            if let Some(chunk_id_bytes) = chunk_id {
                if chunk_id_bytes.len() == 32 {
                    let mut chunk_id_array = [0u8; 32];
                    chunk_id_array.copy_from_slice(&chunk_id_bytes);
                    chunks_to_remove.push(chunk_id_array);
                }
            }
        }
        
        // Remove purged attachments
        db.execute("DELETE FROM attachments WHERE status = 2", [])?;
        
        // Remove chunks (they will be garbage collected by the chunk system)
        let mut chunks_removed = 0;
        for chunk_id in chunks_to_remove {
            let chunk_path = self.chunks_path.join(hex::encode(chunk_id));
            if chunk_path.exists() {
                if let Ok(metadata) = std::fs::metadata(&chunk_path) {
                    space_freed += metadata.len();
                }
                let _ = std::fs::remove_file(&chunk_path);
                chunks_removed += 1;
            }
        }
        
        Ok(super::trait_impl::GarbageCollectionStats {
            chunks_removed,
            attachments_removed,
            space_freed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_temp_sqlite() -> SqliteStorage {
        let dir = TempDir::new().unwrap();
        // Keep directory until test completes to avoid deletion before I/O
        let path = dir.into_path();
        SqliteStorage::open(path).unwrap()
    }

    #[tokio::test]
    async fn test_message_crud() {
        let storage = open_temp_sqlite();

        let msg = crate::types::Message {
            id: crate::types::MessageId::new(),
            content: "hello".to_string(),
            created_at: chrono::Utc::now(),
            is_deleted: false,
            is_purged: false,
        };

        storage.store_message(&msg).await.unwrap();

        let fetched = storage.get_message(&msg.id).await.unwrap().unwrap();
        assert_eq!(fetched.content, "hello");

        let all = storage.get_all_messages().await.unwrap();
        assert_eq!(all.len(), 1);

        storage.delete_message(&msg.id).await.unwrap();
        let all_after = storage.get_all_messages().await.unwrap();
        assert_eq!(all_after.len(), 0);
    }

    #[tokio::test]
    async fn test_chunk_store_and_refs() {
        let storage = open_temp_sqlite();
        let data = b"some-bytes";
        let hash = blake3::hash(data).as_bytes().clone();
        let mut id = [0u8; 32];
        id.copy_from_slice(&hash);

        // Store twice, ref count should increment in DB; file overwrite is fine
        storage.store_chunk(&id, data).await.unwrap();
        storage.store_chunk(&id, data).await.unwrap();

        let exists = storage.has_chunk(&id).await.unwrap();
        assert!(exists);

        let chunk = storage.get_chunk(&id).await.unwrap().unwrap();
        assert_eq!(chunk, data);
    }

    #[tokio::test]
    async fn test_account_keys_and_device_info() {
        let storage = open_temp_sqlite();

        // Account key
        storage.store_account_key(b"encrypted-ak").await.unwrap();
        let ak = storage.get_account_key().await.unwrap().unwrap();
        assert_eq!(ak, b"encrypted-ak");

        // Account key info
        let info = crate::crypto::AccountKeyInfo {
            public_key: [1u8; 32],
            created_at: chrono::Utc::now(),
            expires_at: None,
            version: 1,
            has_private_key: true,
        };
        storage.store_account_key_info(&info).await.unwrap();
        let fetched = storage.get_account_key_info().await.unwrap().unwrap();
        assert_eq!(fetched.public_key, [1u8; 32]);
    }

    #[tokio::test]
    async fn test_authorized_devices_and_device_cert() {
        let storage = open_temp_sqlite();

        // Device cert roundtrip
        let account_key = crate::crypto::AccountKey::generate();
        let device_key = crate::crypto::DeviceKey::generate();
        let cert = crate::crypto::DeviceCert::new(device_key.public_key_bytes(), &account_key, None).unwrap();
        storage.store_device_certificate(&cert).await.unwrap();
        let fetched = storage.get_device_certificate().await.unwrap().unwrap();
        assert_eq!(fetched.device_pubkey, device_key.public_key_bytes());

        // Authorized devices
        storage.store_authorized_device("dev1", b"certbytes").await.unwrap();
        assert!(storage.is_device_authorized("dev1").await.unwrap());
        let list = storage.get_authorized_devices().await.unwrap();
        assert!(list.iter().any(|(id, _)| id == "dev1"));
        storage.revoke_device_authorization("dev1").await.unwrap();
        assert!(!storage.is_device_authorized("dev1").await.unwrap());
    }

}
