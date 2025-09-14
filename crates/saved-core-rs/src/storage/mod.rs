pub mod memory;
pub mod sqlite;
pub mod trait_impl;

pub use memory::MemoryStorage;
pub use sqlite::SqliteStorage;
pub use trait_impl::Storage;

// Re-export types for convenience

/// Storage backend types
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StorageBackend {
    Sqlite,
    Memory,
}

/// Storage configuration
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub path: Option<std::path::PathBuf>, // Only used for SQLite
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::Sqlite,
            path: None,
        }
    }
}

impl StorageConfig {
    pub fn sqlite(path: std::path::PathBuf) -> Self {
        Self {
            backend: StorageBackend::Sqlite,
            path: Some(path),
        }
    }

    pub fn memory() -> Self {
        Self {
            backend: StorageBackend::Memory,
            path: None,
        }
    }
}
