//! Validation utilities

use anyhow::{Result, anyhow};
use std::path::Path;

/// Validate that a path exists and is a directory
pub fn validate_directory_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(anyhow!("Path does not exist: {}", path.display()));
    }
    
    if !path.is_dir() {
        return Err(anyhow!("Path is not a directory: {}", path.display()));
    }
    
    Ok(())
}

/// Validate that a path exists and is a file
pub fn validate_file_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(anyhow!("File does not exist: {}", path.display()));
    }
    
    if !path.is_file() {
        return Err(anyhow!("Path is not a file: {}", path.display()));
    }
    
    Ok(())
}

/// Validate a hex string
pub fn validate_hex_string(hex_str: &str, expected_length: usize) -> Result<()> {
    if hex_str.len() != expected_length * 2 {
        return Err(anyhow!(
            "Hex string must be {} characters long ({} bytes)",
            expected_length * 2,
            expected_length
        ));
    }
    
    if !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(anyhow!("Hex string contains invalid characters"));
    }
    
    Ok(())
}

/// Validate a message ID format
pub fn validate_message_id(message_id: &str) -> Result<()> {
    validate_hex_string(message_id, 32)
}

/// Validate a device ID format
pub fn validate_device_id(device_id: &str) -> Result<()> {
    if device_id.is_empty() {
        return Err(anyhow!("Device ID cannot be empty"));
    }
    
    if device_id.len() > 64 {
        return Err(anyhow!("Device ID too long (max 64 characters)"));
    }
    
    Ok(())
}

/// Validate message content
pub fn validate_message_content(content: &str) -> Result<()> {
    if content.is_empty() {
        return Err(anyhow!("Message content cannot be empty"));
    }
    
    if content.len() > 1_000_000 {
        return Err(anyhow!("Message content too long (max 1MB)"));
    }
    
    Ok(())
}

/// Validate file attachment
pub fn validate_attachment(path: &Path) -> Result<()> {
    validate_file_path(path)?;
    
    // Check file size (max 100MB)
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > 100 * 1024 * 1024 {
        return Err(anyhow!("File too large (max 100MB): {}", path.display()));
    }
    
    Ok(())
}
