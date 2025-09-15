//! Device linking implementation for SAVED
//!
//! Handles secure device linking through QR codes and key exchange protocols.
//! This module provides the foundation for devices to securely share encryption
//! keys and establish trust relationships.

use crate::crypto::{DeviceKey, VaultKey};
use crate::error::{Error, Result};
use crate::networking::NetworkManager;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Device linking request containing the information needed to link a new device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkingRequest {
    /// Device ID of the requesting device
    pub device_id: String,
    /// Device name for display purposes
    pub device_name: String,
    /// Public key for encryption
    pub public_key: Vec<u8>,
    /// Timestamp when the request was created
    pub timestamp: DateTime<Utc>,
    /// Nonce for request validation
    pub nonce: Vec<u8>,
}

/// Device linking response containing the vault key and authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkingResponse {
    /// Encrypted vault key for the new device
    pub encrypted_vault_key: Vec<u8>,
    /// Device authorization token
    pub auth_token: Vec<u8>,
    /// Timestamp when the response was created
    pub timestamp: DateTime<Utc>,
}

/// QR code data for device linking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkingQRData {
    /// The linking request
    pub request: LinkingRequest,
    /// Network endpoint for the linking device
    pub endpoint: String,
    /// Protocol version
    pub version: String,
}

/// Device linking manager
pub struct DeviceLinkingManager {
    /// Pending linking requests
    pending_requests: HashMap<String, LinkingRequest>,
    /// Network manager for communication
    network_manager: Option<NetworkManager>,
}

impl DeviceLinkingManager {
    /// Create a new device linking manager
    pub fn new() -> Self {
        Self {
            pending_requests: HashMap::new(),
            network_manager: None,
        }
    }

    /// Set the network manager for communication
    pub fn set_network_manager(&mut self, network_manager: NetworkManager) {
        self.network_manager = Some(network_manager);
    }

    /// Generate a QR code for device linking
    pub fn generate_linking_qr(
        &mut self,
        device_id: String,
        device_name: String,
        public_key: Vec<u8>,
        endpoint: String,
    ) -> Result<LinkingQRData> {
        let request = LinkingRequest {
            device_id: device_id.clone(),
            device_name,
            public_key,
            timestamp: Utc::now(),
            nonce: self.generate_nonce(),
        };

        let qr_data = LinkingQRData {
            request: request.clone(),
            endpoint,
            version: "1.0.0".to_string(),
        };

        // Store the pending request
        self.pending_requests.insert(device_id, request);

        Ok(qr_data)
    }

    /// Process a linking request from a QR code
    pub async fn process_linking_request(
        &mut self,
        qr_data: LinkingQRData,
        vault_key: VaultKey,
        device_key: DeviceKey,
    ) -> Result<LinkingResponse> {
        // Validate the request
        self.validate_linking_request(&qr_data.request)?;

        // Encrypt the vault key with the requesting device's public key
        let encrypted_vault_key = self.encrypt_vault_key(&qr_data.request.public_key, &vault_key)?;

        // Generate authorization token
        let auth_token = self.generate_auth_token(&qr_data.request, &device_key)?;

        let response = LinkingResponse {
            encrypted_vault_key,
            auth_token,
            timestamp: Utc::now(),
        };

        // Send the response to the requesting device
        let device_id = qr_data.request.device_id.clone();
        if let Some(network_manager) = &mut self.network_manager {
            Self::send_linking_response(network_manager, &device_id, &response).await?;
        }

        Ok(response)
    }

    /// Accept a linking response and complete the device linking
    pub async fn accept_linking_response(
        &mut self,
        device_id: String,
        response: LinkingResponse,
        device_key: DeviceKey,
    ) -> Result<VaultKey> {
        // Find the pending request
        let request = self.pending_requests.get(&device_id)
            .ok_or_else(|| Error::DeviceLinking("No pending request found".to_string()))?;

        // Validate the response
        self.validate_linking_response(request, &response, &device_key)?;

        // Decrypt the vault key
        let vault_key = self.decrypt_vault_key(&response.encrypted_vault_key, &device_key)?;

        // Remove the pending request
        self.pending_requests.remove(&device_id);

        Ok(vault_key)
    }

    /// Generate a nonce for request validation
    fn generate_nonce(&self) -> Vec<u8> {
        use rand::RngCore;
        let mut nonce = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut nonce);
        nonce
    }

    /// Validate a linking request
    fn validate_linking_request(&self, request: &LinkingRequest) -> Result<()> {
        // Check timestamp (request should be recent)
        let now = Utc::now();
        let age = now.signed_duration_since(request.timestamp);
        if age.num_minutes() > 10 {
            return Err(Error::DeviceLinking("Linking request expired".to_string()));
        }

        // Validate nonce
        if request.nonce.len() != 32 {
            return Err(Error::DeviceLinking("Invalid nonce length".to_string()));
        }

        // Validate public key
        if request.public_key.is_empty() {
            return Err(Error::DeviceLinking("Empty public key".to_string()));
        }

        Ok(())
    }

    /// Encrypt vault key with device's public key
    fn encrypt_vault_key(&self, public_key: &[u8], _vault_key: &VaultKey) -> Result<Vec<u8>> {
        // TODO: Implement proper encryption with the device's public key
        // For now, return a placeholder
        Ok(format!("encrypted_vault_key_for_{}", hex::encode(&public_key[..8])).into_bytes())
    }

    /// Decrypt vault key with device's private key
    fn decrypt_vault_key(&self, _encrypted_vault_key: &[u8], _device_key: &DeviceKey) -> Result<VaultKey> {
        // TODO: Implement proper decryption with the device's private key
        // For now, return a placeholder vault key
        Ok([0u8; 32])
    }

    /// Generate authorization token
    fn generate_auth_token(&self, request: &LinkingRequest, _device_key: &DeviceKey) -> Result<Vec<u8>> {
        // TODO: Implement proper token generation and signing
        // For now, return a placeholder
        Ok(format!("auth_token_for_{}", request.device_id).into_bytes())
    }

    /// Validate linking response
    fn validate_linking_response(
        &self,
        _request: &LinkingRequest,
        response: &LinkingResponse,
        _device_key: &DeviceKey,
    ) -> Result<()> {
        // Check timestamp
        let now = Utc::now();
        let age = now.signed_duration_since(response.timestamp);
        if age.num_minutes() > 5 {
            return Err(Error::DeviceLinking("Linking response expired".to_string()));
        }

        // TODO: Validate the authorization token signature
        // For now, just check that it's not empty
        if response.auth_token.is_empty() {
            return Err(Error::DeviceLinking("Empty authorization token".to_string()));
        }

        Ok(())
    }

    /// Send linking response to requesting device
    async fn send_linking_response(
        _network_manager: &mut NetworkManager,
        device_id: &str,
        _response: &LinkingResponse,
    ) -> Result<()> {
        // TODO: Implement proper network communication
        // For now, just log the response
        println!("Sending linking response to device: {}", device_id);
        Ok(())
    }

    /// Get pending linking requests
    pub fn get_pending_requests(&self) -> Vec<&LinkingRequest> {
        self.pending_requests.values().collect()
    }

    /// Cancel a pending linking request
    pub fn cancel_linking_request(&mut self, device_id: &str) -> bool {
        self.pending_requests.remove(device_id).is_some()
    }
}

impl Default for DeviceLinkingManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linking_qr_generation() {
        let mut manager = DeviceLinkingManager::new();
        let device_id = "test-device-123".to_string();
        let device_name = "Test Device".to_string();
        let public_key = vec![1, 2, 3, 4, 5];
        let endpoint = "127.0.0.1:8080".to_string();

        let qr_data = manager.generate_linking_qr(
            device_id.clone(),
            device_name,
            public_key,
            endpoint,
        ).unwrap();

        assert_eq!(qr_data.request.device_id, device_id);
        assert_eq!(qr_data.version, "1.0.0");
        assert!(!qr_data.request.nonce.is_empty());
    }

    #[test]
    fn test_pending_requests() {
        let mut manager = DeviceLinkingManager::new();
        let device_id = "test-device-456".to_string();
        let device_name = "Test Device".to_string();
        let public_key = vec![1, 2, 3, 4, 5];
        let endpoint = "127.0.0.1:8080".to_string();

        // Generate QR code
        manager.generate_linking_qr(
            device_id.clone(),
            device_name,
            public_key,
            endpoint,
        ).unwrap();

        // Check pending requests
        let pending = manager.get_pending_requests();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].device_id, device_id);

        // Cancel request
        let cancelled = manager.cancel_linking_request(&device_id);
        assert!(cancelled);

        // Check no pending requests
        let pending = manager.get_pending_requests();
        assert_eq!(pending.len(), 0);
    }
}
