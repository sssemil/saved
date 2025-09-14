//! SQLite storage implementation for SAVED
//!
//! Implements:
//! - SQLite database for metadata and indices (WAL mode)
//! - Content-addressed chunk store for encrypted file attachments
//! - Reference counting for garbage collection

use crate::error::Result;
use crate::events::{Op, OpHash, EventLog};
use crate::types::{MessageId, Message};
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;
use async_trait::async_trait;

use super::trait_impl::{Storage, StorageStats};

/// Chunk identifier (BLAKE3 hash of plaintext)
pub type ChunkId = [u8; 32];

/// SQLite storage implementation
pub struct SqliteStorage {
    /// SQLite connection (thread-safe)
    db: Arc<Mutex<Connection>>,
    /// Path to the account directory
    account_path: PathBuf,
    /// Path to the chunks directory
    chunks_path: PathBuf,
    /// In-memory event log
    event_log: EventLog,
    /// Reference counts for chunks
    chunk_refs: HashMap<ChunkId, u32>,
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
        let mut db = Connection::open(db_path)?;
        
        // Enable WAL mode for better concurrency
        db.execute("PRAGMA journal_mode=WAL", [])?;
        db.execute("PRAGMA synchronous=NORMAL", [])?;
        db.execute("PRAGMA cache_size=10000", [])?;
        db.execute("PRAGMA temp_store=memory", [])?;
        
        let mut storage = Self {
            db: Arc::new(Mutex::new(db)),
            account_path,
            chunks_path,
            event_log: EventLog::new(),
            chunk_refs: HashMap::new(),
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

        for row in rows {
            let (chunk_id_bytes, ref_count) = row?;
            if chunk_id_bytes.len() == 32 {
                let mut chunk_id = [0u8; 32];
                chunk_id.copy_from_slice(&chunk_id_bytes);
                self.chunk_refs.insert(chunk_id, ref_count);
            }
        }

        Ok(())
    }

    /// Update chunk reference count
    fn update_chunk_ref(&mut self, chunk_id: &ChunkId, delta: i32) -> Result<()> {
        let current_refs = self.chunk_refs.get(chunk_id).copied().unwrap_or(0);
        let new_refs = (current_refs as i32 + delta).max(0) as u32;

        let db = self.db.lock().unwrap();
        if new_refs == 0 {
            self.chunk_refs.remove(chunk_id);
            db.execute(
                "DELETE FROM chunk_refs WHERE chunk_id = ?",
                params![chunk_id.as_slice()],
            )?;
        } else {
            self.chunk_refs.insert(*chunk_id, new_refs);
            db.execute(
                "INSERT OR REPLACE INTO chunk_refs (chunk_id, ref_count) VALUES (?, ?)",
                params![chunk_id.as_slice(), new_refs],
            )?;
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
                bincode::serialize(&operation.parents).map_err(|e| crate::error::Error::Sync(format!("Serialization error: {}", e)))?,
                bincode::serialize(&operation.operation).map_err(|e| crate::error::Error::Sync(format!("Operation serialization error: {}", e)))?,
                operation.timestamp.timestamp()
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
            let hash_bytes: Vec<u8> = row.get(0)?;
            let op_id_bytes: Vec<u8> = row.get(1)?;
            let device_pubkey_bytes: Vec<u8> = row.get(2)?;
            let counter: u64 = row.get(3)?;
            let lamport: u64 = row.get(4)?;
            let parents_bytes: Vec<u8> = row.get(5)?;
            let operation_data: Vec<u8> = row.get(6)?;
            let timestamp: i64 = row.get(7)?;

            // Convert bytes back to proper types
            let mut device_pubkey = [0u8; 32];
            device_pubkey.copy_from_slice(&device_pubkey_bytes[..32]);
            
            let op_id = crate::events::OpId::from_bytes(&op_id_bytes).map_err(|e| rusqlite::Error::InvalidParameterName(format!("OpId deserialization error: {}", e)))?;
            let parents: Vec<crate::events::OpHash> = bincode::deserialize(&parents_bytes).map_err(|e| rusqlite::Error::InvalidParameterName(format!("Deserialization error: {}", e)))?;
            let operation: crate::events::Operation = bincode::deserialize(&operation_data).map_err(|e| rusqlite::Error::InvalidParameterName(format!("Operation deserialization error: {}", e)))?;

            Ok(Op {
                id: op_id,
                lamport,
                parents,
                operation,
                timestamp: chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_default().with_timezone(&chrono::Utc),
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
            let hash_bytes: Vec<u8> = row.get(0)?;
            let op_id_bytes: Vec<u8> = row.get(1)?;
            let device_pubkey_bytes: Vec<u8> = row.get(2)?;
            let counter: u64 = row.get(3)?;
            let lamport: u64 = row.get(4)?;
            let parents_bytes: Vec<u8> = row.get(5)?;
            let operation_data: Vec<u8> = row.get(6)?;
            let timestamp: i64 = row.get(7)?;

            // Convert bytes back to proper types
            let mut device_pubkey = [0u8; 32];
            device_pubkey.copy_from_slice(&device_pubkey_bytes[..32]);
            
            let op_id = crate::events::OpId::from_bytes(&op_id_bytes).map_err(|e| rusqlite::Error::InvalidParameterName(format!("OpId deserialization error: {}", e)))?;
            let parents: Vec<crate::events::OpHash> = bincode::deserialize(&parents_bytes).map_err(|e| rusqlite::Error::InvalidParameterName(format!("Deserialization error: {}", e)))?;
            let operation: crate::events::Operation = bincode::deserialize(&operation_data).map_err(|e| rusqlite::Error::InvalidParameterName(format!("Operation deserialization error: {}", e)))?;

            Ok(Op {
                id: op_id,
                lamport,
                parents,
                operation,
                timestamp: chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_default().with_timezone(&chrono::Utc),
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
            "SELECT id, content, created_at, is_deleted, is_purged FROM messages WHERE id = ?"
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
}
