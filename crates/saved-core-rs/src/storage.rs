//! Storage layer for SAVED
//!
//! Implements:
//! - SQLite database for metadata and indices (WAL mode)
//! - Content-addressed chunk store for encrypted file attachments
//! - Reference counting for garbage collection

use crate::error::Result;
use crate::events::{Op, OpHash, EventLog};
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use walkdir::WalkDir;

/// Chunk identifier (BLAKE3 hash of plaintext)
pub type ChunkId = [u8; 32];

/// Storage manager for SAVED account data
pub struct Storage {
    /// SQLite connection
    db: Connection,
    /// Path to the account directory
    account_path: PathBuf,
    /// Path to the chunks directory
    chunks_path: PathBuf,
    /// In-memory event log
    event_log: EventLog,
    /// Reference counts for chunks
    chunk_refs: HashMap<ChunkId, u32>,
}

impl Storage {
    /// Open or create storage at the given path
    pub fn open(account_path: PathBuf) -> Result<Self> {
        // Ensure account directory exists
        fs::create_dir_all(&account_path)?;
        
        let chunks_path = account_path.join("chunks");
        fs::create_dir_all(&chunks_path)?;
        
        // Open SQLite database
        let db_path = account_path.join("db.sqlite");
        let mut db = Connection::open(db_path)?;
        
        // Enable WAL mode for better concurrency
        db.execute("PRAGMA journal_mode=WAL", [])?;
        db.execute("PRAGMA synchronous=NORMAL", [])?;
        db.execute("PRAGMA cache_size=10000", [])?;
        db.execute("PRAGMA temp_store=memory", [])?;
        
        let mut storage = Self {
            db,
            account_path,
            chunks_path,
            event_log: EventLog::new(),
            chunk_refs: HashMap::new(),
        };
        
        // Initialize database schema
        storage.init_schema()?;
        
        // Load existing data
        storage.load_event_log()?;
        storage.load_chunk_refs()?;
        
        Ok(storage)
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<()> {
        // Operations table
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS operations (
                hash BLOB PRIMARY KEY,
                op_id BLOB NOT NULL,
                device_pubkey BLOB NOT NULL,
                counter INTEGER NOT NULL,
                lamport INTEGER NOT NULL,
                parents BLOB NOT NULL,
                operation_type TEXT NOT NULL,
                operation_data BLOB NOT NULL,
                timestamp INTEGER NOT NULL,
                UNIQUE(device_pubkey, counter)
            )",
            [],
        )?;

        // Messages table (materialized view)
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                msg_id BLOB PRIMARY KEY,
                feed_id TEXT NOT NULL,
                body TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                edited_at INTEGER,
                deleted_at INTEGER,
                author_device BLOB NOT NULL,
                is_purged BOOLEAN DEFAULT FALSE
            )",
            [],
        )?;

        // Attachments table
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS attachments (
                msg_id BLOB NOT NULL,
                chunk_id BLOB NOT NULL,
                filename TEXT,
                mime_type TEXT,
                size INTEGER,
                PRIMARY KEY (msg_id, chunk_id),
                FOREIGN KEY (msg_id) REFERENCES messages (msg_id)
            )",
            [],
        )?;

        // Chunk references table
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS chunk_refs (
                chunk_id BLOB PRIMARY KEY,
                ref_count INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        // Device certificates table
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS device_certs (
                device_pubkey BLOB PRIMARY KEY,
                cert_data BLOB NOT NULL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        // Create indexes
        self.db.execute(
            "CREATE INDEX IF NOT EXISTS idx_operations_device_counter ON operations (device_pubkey, counter)",
            [],
        )?;
        self.db.execute(
            "CREATE INDEX IF NOT EXISTS idx_operations_lamport ON operations (lamport)",
            [],
        )?;
        self.db.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_feed ON messages (feed_id)",
            [],
        )?;
        self.db.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_created ON messages (created_at)",
            [],
        )?;

        Ok(())
    }

    /// Load event log from database
    fn load_event_log(&mut self) -> Result<()> {
        let mut stmt = self.db.prepare(
            "SELECT hash, op_id, device_pubkey, counter, lamport, parents, 
                    operation_type, operation_data, timestamp 
             FROM operations ORDER BY lamport"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Vec<u8>>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;

        for row in rows {
            let (hash_bytes, op_id_bytes, device_pubkey_bytes, counter, lamport, 
                 parents_bytes, operation_type, operation_data, timestamp) = row?;
            
            // Reconstruct operation
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&hash_bytes[0..32]);
            
            let op_id = crate::events::OpId::from_bytes(&op_id_bytes)?;
            
            let mut device_pubkey = [0u8; 32];
            device_pubkey.copy_from_slice(&device_pubkey_bytes[0..32]);
            
            let parents: Vec<OpHash> = serde_json::from_slice(&parents_bytes)?;
            let operation: crate::events::Operation = serde_json::from_slice(&operation_data)?;
            
            let op = Op::new(op_id, lamport as u64, parents, operation);
            self.event_log.add_operation(op)?;
        }

        Ok(())
    }

    /// Load chunk reference counts from database
    fn load_chunk_refs(&mut self) -> Result<()> {
        let mut stmt = self.db.prepare("SELECT chunk_id, ref_count FROM chunk_refs")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, i64>(1)?,
            ))
        })?;

        for row in rows {
            let (chunk_id_bytes, ref_count) = row?;
            let mut chunk_id = [0u8; 32];
            chunk_id.copy_from_slice(&chunk_id_bytes[0..32]);
            self.chunk_refs.insert(chunk_id, ref_count as u32);
        }

        Ok(())
    }

    /// Store an operation in the database
    pub fn store_operation(&mut self, op: &Op) -> Result<()> {
        let hash = op.hash();
        let hash_bytes = hash.to_vec();
        let op_id_bytes = op.id.to_bytes();
        let device_pubkey_bytes = op.id.device_pubkey.to_vec();
        let parents_bytes = serde_json::to_vec(&op.parents)?;
        let operation_data = serde_json::to_vec(&op.operation)?;
        let timestamp = op.timestamp.timestamp();

        self.db.execute(
            "INSERT OR REPLACE INTO operations 
             (hash, op_id, device_pubkey, counter, lamport, parents, 
              operation_type, operation_data, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                hash_bytes,
                op_id_bytes,
                device_pubkey_bytes,
                op.id.counter as i64,
                op.lamport as i64,
                parents_bytes,
                self.operation_type_name(&op.operation),
                operation_data,
                timestamp
            ],
        )?;

        // Update materialized views
        self.update_materialized_views(op)?;

        Ok(())
    }

    /// Get operation type name for storage
    fn operation_type_name(&self, operation: &crate::events::Operation) -> String {
        match operation {
            crate::events::Operation::CreateMessage { .. } => "CreateMessage".to_string(),
            crate::events::Operation::EditMessage { .. } => "EditMessage".to_string(),
            crate::events::Operation::DeleteMessage { .. } => "DeleteMessage".to_string(),
            crate::events::Operation::Attach { .. } => "Attach".to_string(),
            crate::events::Operation::Detach { .. } => "Detach".to_string(),
            crate::events::Operation::Ack { .. } => "Ack".to_string(),
            crate::events::Operation::Purge { .. } => "Purge".to_string(),
        }
    }

    /// Update materialized views after operation
    fn update_materialized_views(&mut self, op: &Op) -> Result<()> {
        match &op.operation {
            crate::events::Operation::CreateMessage { msg_id, feed_id, body, attachments, created_at } => {
                self.db.execute(
                    "INSERT OR REPLACE INTO messages 
                     (msg_id, feed_id, body, created_at, author_device)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        msg_id.to_vec(),
                        feed_id,
                        body,
                        created_at.timestamp(),
                        op.id.device_pubkey.to_vec()
                    ],
                )?;

                // Add attachment references
                for chunk_id in attachments {
                    self.increment_chunk_ref(*chunk_id)?;
                    self.db.execute(
                        "INSERT OR IGNORE INTO attachments (msg_id, chunk_id)
                         VALUES (?1, ?2)",
                        params![msg_id.to_vec(), chunk_id.to_vec()],
                    )?;
                }
            }
            crate::events::Operation::EditMessage { msg_id, body, edited_at } => {
                self.db.execute(
                    "UPDATE messages SET body = ?1, edited_at = ?2 WHERE msg_id = ?3",
                    params![body, edited_at.timestamp(), msg_id.to_vec()],
                )?;
            }
            crate::events::Operation::DeleteMessage { msg_id, deleted_at, .. } => {
                self.db.execute(
                    "UPDATE messages SET deleted_at = ?1 WHERE msg_id = ?2",
                    params![deleted_at.timestamp(), msg_id.to_vec()],
                )?;
            }
            crate::events::Operation::Attach { msg_id, attachment_cids } => {
                for chunk_id in attachment_cids {
                    self.increment_chunk_ref(*chunk_id)?;
                    self.db.execute(
                        "INSERT OR IGNORE INTO attachments (msg_id, chunk_id)
                         VALUES (?1, ?2)",
                        params![msg_id.to_vec(), chunk_id.to_vec()],
                    )?;
                }
            }
            crate::events::Operation::Detach { msg_id, attachment_cids } => {
                for chunk_id in attachment_cids {
                    self.decrement_chunk_ref(*chunk_id)?;
                    self.db.execute(
                        "DELETE FROM attachments WHERE msg_id = ?1 AND chunk_id = ?2",
                        params![msg_id.to_vec(), chunk_id.to_vec()],
                    )?;
                }
            }
            crate::events::Operation::Purge { msg_id, .. } => {
                // Mark message as purged
                self.db.execute(
                    "UPDATE messages SET is_purged = TRUE WHERE msg_id = ?1",
                    params![msg_id.to_vec()],
                )?;

                // Remove all attachments and decrement refs
                let chunk_ids: Vec<[u8; 32]> = {
                    let mut stmt = self.db.prepare(
                        "SELECT chunk_id FROM attachments WHERE msg_id = ?1"
                    )?;
                    let rows = stmt.query_map(params![msg_id.to_vec()], |row| {
                        Ok(row.get::<_, Vec<u8>>(0)?)
                    })?;

                    let mut chunk_ids = Vec::new();
                    for row in rows {
                        let chunk_id_bytes = row?;
                        let mut chunk_id = [0u8; 32];
                        chunk_id.copy_from_slice(&chunk_id_bytes[0..32]);
                        chunk_ids.push(chunk_id);
                    }
                    chunk_ids
                };
                
                // Decrement refs after dropping the statement
                for chunk_id in chunk_ids {
                    self.decrement_chunk_ref(chunk_id)?;
                }

                self.db.execute(
                    "DELETE FROM attachments WHERE msg_id = ?1",
                    params![msg_id.to_vec()],
                )?;
            }
            _ => {} // Other operations don't affect materialized views
        }

        Ok(())
    }

    /// Store a chunk in the content-addressed store
    pub fn store_chunk(&mut self, chunk_id: ChunkId, data: &[u8]) -> Result<()> {
        let chunk_path = self.get_chunk_path(chunk_id);
        
        // Create directory if it doesn't exist
        if let Some(parent) = chunk_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Write chunk data
        fs::write(&chunk_path, data)?;
        
        // Update reference count
        self.db.execute(
            "INSERT OR REPLACE INTO chunk_refs (chunk_id, ref_count, size, created_at)
             VALUES (?1, COALESCE((SELECT ref_count FROM chunk_refs WHERE chunk_id = ?1), 0), ?2, ?3)",
            params![
                chunk_id.to_vec(),
                data.len() as i64,
                chrono::Utc::now().timestamp()
            ],
        )?;

        Ok(())
    }

    /// Retrieve a chunk from the content-addressed store
    pub fn get_chunk(&self, chunk_id: ChunkId) -> Result<Vec<u8>> {
        let chunk_path = self.get_chunk_path(chunk_id);
        fs::read(&chunk_path).map_err(|e| e.into())
    }

    /// Check if a chunk exists
    pub fn has_chunk(&self, chunk_id: ChunkId) -> bool {
        let chunk_path = self.get_chunk_path(chunk_id);
        chunk_path.exists()
    }

    /// Get the path for a chunk
    fn get_chunk_path(&self, chunk_id: ChunkId) -> PathBuf {
        let hex = hex::encode(chunk_id);
        let prefix1 = &hex[0..2];
        let prefix2 = &hex[2..4];
        self.chunks_path.join(prefix1).join(prefix2).join(&hex)
    }

    /// Increment chunk reference count
    fn increment_chunk_ref(&mut self, chunk_id: ChunkId) -> Result<()> {
        let count = self.chunk_refs.entry(chunk_id).or_insert(0);
        *count += 1;
        
        self.db.execute(
            "INSERT OR REPLACE INTO chunk_refs (chunk_id, ref_count)
             VALUES (?1, ?2)",
            params![chunk_id.to_vec(), *count as i64],
        )?;

        Ok(())
    }

    /// Decrement chunk reference count and delete if zero
    fn decrement_chunk_ref(&mut self, chunk_id: ChunkId) -> Result<()> {
        if let Some(count) = self.chunk_refs.get_mut(&chunk_id) {
            if *count > 0 {
                *count -= 1;
                
                if *count == 0 {
                    // Delete the chunk file
                    let chunk_path = self.get_chunk_path(chunk_id);
                    if chunk_path.exists() {
                        fs::remove_file(&chunk_path)?;
                    }
                    
                    // Remove from database
                    self.db.execute(
                        "DELETE FROM chunk_refs WHERE chunk_id = ?1",
                        params![chunk_id.to_vec()],
                    )?;
                    
                    self.chunk_refs.remove(&chunk_id);
                } else {
                    // Update reference count
                    self.db.execute(
                        "UPDATE chunk_refs SET ref_count = ?1 WHERE chunk_id = ?2",
                        params![*count as i64, chunk_id.to_vec()],
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Get the event log
    pub fn event_log(&self) -> &EventLog {
        &self.event_log
    }

    /// Get mutable access to the event log
    pub fn event_log_mut(&mut self) -> &mut EventLog {
        &mut self.event_log
    }

    /// Get all messages for a feed
    pub fn get_messages(&self, feed_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self.db.prepare(
            "SELECT msg_id, feed_id, body, created_at, edited_at, deleted_at, 
                    author_device, is_purged
             FROM messages 
             WHERE feed_id = ?1 AND is_purged = FALSE
             ORDER BY created_at DESC"
        )?;

        let rows = stmt.query_map(params![feed_id], |row| {
            Ok(Message {
                msg_id: {
                    let mut id = [0u8; 32];
                    let bytes: Vec<u8> = row.get(0)?;
                    id.copy_from_slice(&bytes[0..32]);
                    id
                },
                feed_id: row.get(1)?,
                body: row.get(2)?,
                created_at: chrono::DateTime::from_timestamp(row.get::<_, i64>(3)?, 0)
                    .unwrap_or_default(),
                edited_at: row.get::<_, Option<i64>>(4)?
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)),
                deleted_at: row.get::<_, Option<i64>>(5)?
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)),
                author_device: {
                    let mut device = [0u8; 32];
                    let bytes: Vec<u8> = row.get(6)?;
                    device.copy_from_slice(&bytes[0..32]);
                    device
                },
                is_purged: row.get(7)?,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }

        Ok(messages)
    }

    /// Get total storage size
    pub fn get_storage_size(&self) -> Result<u64> {
        let mut total_size = 0u64;
        
        // Database size
        let db_path = self.account_path.join("db.sqlite");
        if db_path.exists() {
            total_size += fs::metadata(&db_path)?.len();
        }
        
        // Chunks size
        for entry in WalkDir::new(&self.chunks_path) {
            let entry = entry?;
            if entry.file_type().is_file() {
                total_size += entry.metadata()?.len();
            }
        }
        
        Ok(total_size)
    }
}

/// Message structure for materialized view
#[derive(Debug, Clone)]
pub struct Message {
    pub msg_id: [u8; 32],
    pub feed_id: String,
    pub body: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub edited_at: Option<chrono::DateTime<chrono::Utc>>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub author_device: [u8; 32],
    pub is_purged: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_storage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path().to_path_buf()).unwrap();
        assert!(storage.event_log().get_heads().is_empty());
    }

    #[test]
    fn test_chunk_storage() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = Storage::open(temp_dir.path().to_path_buf()).unwrap();
        
        let data = b"Hello, SAVED!";
        let chunk_id = blake3_hash(data);
        
        storage.store_chunk(chunk_id, data).unwrap();
        assert!(storage.has_chunk(chunk_id));
        
        let retrieved = storage.get_chunk(chunk_id).unwrap();
        assert_eq!(data, &retrieved[..]);
    }
}
