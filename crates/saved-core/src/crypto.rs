//! Cryptography module for SAVED
//!
//! Implements:
//! - ed25519 keys for account and device authentication
//! - XChaCha20-Poly1305 encryption for event payloads
//! - BLAKE3 for content addressing and hashing
//! - HKDF for key derivation
//! - Argon2id for passphrase-based key protection

use crate::error::{Error, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use base64::{engine::general_purpose, Engine as _};
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use sha2::Sha256;

/// 32-byte vault key for encrypting event payloads and chunk keys
pub type VaultKey = [u8; 32];

/// 24-byte nonce for XChaCha20-Poly1305
pub type NonceBytes = [u8; 24];

/// Account key (ed25519) - long-lived, can be shared between devices
#[derive(Debug, Clone)]
pub struct AccountKey {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

/// Account key metadata for distributed management
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountKeyInfo {
    /// The account public key
    pub public_key: [u8; 32],
    /// When this account key was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Optional expiration time
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Version number for key rotation
    pub version: u64,
    /// Whether this device has the private key
    pub has_private_key: bool,
}

impl AccountKey {
    /// Generate a new account key
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Create from existing key material
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let signing_key = SigningKey::from_bytes(bytes);
        let verifying_key = signing_key.verifying_key();
        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Get the public key bytes
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    /// Get the private key bytes
    pub fn private_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Sign data with the account key
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }

    /// Verify a signature with the account key
    pub fn verify(&self, data: &[u8], signature: &Signature) -> Result<()> {
        self.verifying_key
            .verify(data, signature)
            .map_err(|e| Error::Crypto(format!("Signature verification failed: {}", e)))
    }

    /// Create account key info for this key
    pub fn to_info(&self, version: u64, has_private_key: bool) -> AccountKeyInfo {
        AccountKeyInfo {
            public_key: self.public_key_bytes(),
            created_at: chrono::Utc::now(),
            expires_at: None,
            version,
            has_private_key,
        }
    }

    /// Create account key info with expiration
    pub fn to_info_with_expiration(
        &self,
        version: u64,
        has_private_key: bool,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> AccountKeyInfo {
        AccountKeyInfo {
            public_key: self.public_key_bytes(),
            created_at: chrono::Utc::now(),
            expires_at: Some(expires_at),
            version,
            has_private_key,
        }
    }

    /// Create a new account key from existing private key bytes
    pub fn from_private_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let signing_key = SigningKey::from_bytes(bytes);
        let verifying_key = signing_key.verifying_key();
        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Create account key from public key only (for verification)
    pub fn from_public_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let verifying_key = VerifyingKey::from_bytes(bytes)
            .map_err(|e| Error::Crypto(format!("Invalid public key: {}", e)))?;
        // We can't create a signing key from public key, so we'll use a dummy one
        // This should only be used for verification
        let signing_key = SigningKey::generate(&mut OsRng);
        Ok(Self {
            signing_key,
            verifying_key,
        })
    }
}

/// Device key (ed25519) - per device, authenticates in libp2p
#[derive(Debug, Clone)]
pub struct DeviceKey {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl DeviceKey {
    /// Generate a new device key
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Create from existing key material
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let signing_key = SigningKey::from_bytes(bytes);
        let verifying_key = signing_key.verifying_key();
        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Get the public key bytes
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    /// Get the private key bytes
    pub fn private_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Create from signing and verifying keys
    pub fn from_keys(signing_key: SigningKey, verifying_key: VerifyingKey) -> Self {
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Sign data with the device key
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }

    /// Verify a signature with the device key
    pub fn verify(&self, data: &[u8], signature: &Signature) -> Result<()> {
        self.verifying_key
            .verify(data, signature)
            .map_err(|e| Error::Crypto(format!("Signature verification failed: {}", e)))
    }
}

/// Device certificate signed by account key
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceCert {
    /// Device public key
    pub device_pubkey: [u8; 32],
    /// When the certificate was issued
    pub issued_at: chrono::DateTime<chrono::Utc>,
    /// Optional expiration time
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Signature by the account key
    pub signature: Vec<u8>,
}

impl DeviceCert {
    /// Create a new device certificate
    pub fn new(
        device_pubkey: [u8; 32],
        account_key: &AccountKey,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Self> {
        let issued_at = chrono::Utc::now();

        // Create the data to be signed
        let mut data = Vec::new();
        data.extend_from_slice(&device_pubkey);
        data.extend_from_slice(&issued_at.timestamp().to_le_bytes());
        if let Some(expires) = expires_at {
            data.extend_from_slice(&expires.timestamp().to_le_bytes());
        }

        let signature = account_key.sign(&data);

        Ok(Self {
            device_pubkey,
            issued_at,
            expires_at,
            signature: signature.to_bytes().to_vec(),
        })
    }

    /// Verify the device certificate
    pub fn verify(&self, account_key: &AccountKey) -> Result<()> {
        // Recreate the data that was signed
        let mut data = Vec::new();
        data.extend_from_slice(&self.device_pubkey);
        data.extend_from_slice(&self.issued_at.timestamp().to_le_bytes());
        if let Some(expires) = self.expires_at {
            data.extend_from_slice(&expires.timestamp().to_le_bytes());
        }

        if self.signature.len() != 64 {
            return Err(Error::Crypto("Invalid signature length".to_string()));
        }
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&self.signature[..]);
        let signature = Signature::from_bytes(&sig_bytes);

        account_key.verify(&data, &signature)
    }
}

/// Generate a random vault key
pub fn generate_vault_key() -> VaultKey {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

/// Generate a random nonce
pub fn generate_nonce() -> NonceBytes {
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Derive an event encryption key from the vault key and operation ID
pub fn derive_event_key(vault_key: &VaultKey, op_id: &[u8]) -> Result<[u8; 32]> {
    let hkdf = Hkdf::<Sha256>::new(None, vault_key);
    let mut key = [0u8; 32];
    let mut info = b"event".to_vec();
    info.extend_from_slice(op_id);
    hkdf.expand(&info, &mut key)
        .map_err(|e| Error::Crypto(format!("HKDF expansion failed: {}", e)))?;
    Ok(key)
}

/// Derive a chunk encryption key for convergent encryption
pub fn derive_chunk_key(vault_key: &VaultKey, plaintext_hash: &[u8]) -> Result<[u8; 32]> {
    let hkdf = Hkdf::<Sha256>::new(None, vault_key);
    let mut key = [0u8; 32];
    let mut info = b"chunk".to_vec();
    info.extend_from_slice(plaintext_hash);
    hkdf.expand(&info, &mut key)
        .map_err(|e| Error::Crypto(format!("HKDF expansion failed: {}", e)))?;
    Ok(key)
}

/// Encrypt data with XChaCha20-Poly1305
pub fn encrypt(key: &[u8; 32], nonce: &NonceBytes, plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .encrypt(nonce.into(), plaintext)
        .map_err(|e| Error::Crypto(format!("Encryption failed: {}", e)))
}

/// Decrypt data with XChaCha20-Poly1305
pub fn decrypt(key: &[u8; 32], nonce: &NonceBytes, ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(nonce.into(), ciphertext)
        .map_err(|e| Error::Crypto(format!("Decryption failed: {}", e)))
}

/// Compute BLAKE3 hash of data
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

/// Protect a vault key with a passphrase using Argon2id
pub fn protect_vault_key_with_passphrase(vault_key: &VaultKey, passphrase: &str) -> Result<String> {
    let salt = generate_nonce(); // Use 24 bytes for salt
    let argon2 = Argon2::default();

    let salt_string = SaltString::encode_b64(&salt)
        .map_err(|e| Error::Crypto(format!("Salt encoding failed: {}", e)))?;
    let password_hash = argon2
        .hash_password(passphrase.as_bytes(), &salt_string)
        .map_err(|e| Error::Crypto(format!("Argon2 hashing failed: {}", e)))?;

    // Encrypt the vault key with the derived key
    let hash = password_hash.hash.unwrap();
    let derived_key = hash.as_bytes();
    if derived_key.len() != 32 {
        return Err(Error::Crypto("Invalid derived key length".to_string()));
    }
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(derived_key);
    let nonce = generate_nonce();
    let encrypted_key = encrypt(&key_bytes, &nonce, vault_key)?;

    // Combine salt, nonce, and encrypted key
    let mut result = Vec::new();
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&encrypted_key);

    Ok(general_purpose::STANDARD.encode(result))
}

/// Recover a vault key from a passphrase-protected string
pub fn recover_vault_key_from_passphrase(protected: &str, passphrase: &str) -> Result<VaultKey> {
    let data = general_purpose::STANDARD
        .decode(protected)
        .map_err(|e| Error::Crypto(format!("Base64 decode failed: {}", e)))?;

    if data.len() < 24 + 24 + 32 {
        return Err(Error::Crypto("Invalid protected key format".to_string()));
    }

    let salt = &data[0..24];
    let nonce = &data[24..48];
    let encrypted_key = &data[48..];

    let argon2 = Argon2::default();
    let salt_string = SaltString::encode_b64(salt)
        .map_err(|e| Error::Crypto(format!("Salt encoding failed: {}", e)))?;
    let password_hash = argon2
        .hash_password(passphrase.as_bytes(), &salt_string)
        .map_err(|e| Error::Crypto(format!("Argon2 hashing failed: {}", e)))?;

    let hash = password_hash.hash.unwrap();
    let derived_key = hash.as_bytes();
    if derived_key.len() != 32 {
        return Err(Error::Crypto("Invalid derived key length".to_string()));
    }
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(derived_key);
    let decrypted_key = decrypt(&key_bytes, nonce.try_into().unwrap(), encrypted_key)?;

    if decrypted_key.len() != 32 {
        return Err(Error::Crypto("Invalid decrypted key length".to_string()));
    }

    let mut vault_key = [0u8; 32];
    vault_key.copy_from_slice(&decrypted_key);
    Ok(vault_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let account_key = AccountKey::generate();
        let device_key = DeviceKey::generate();
        let vault_key = generate_vault_key();

        assert_eq!(account_key.public_key_bytes().len(), 32);
        assert_eq!(device_key.public_key_bytes().len(), 32);
        assert_eq!(vault_key.len(), 32);
    }

    #[test]
    fn test_encryption_decryption() {
        let key = generate_vault_key();
        let nonce = generate_nonce();
        let plaintext = b"Hello, SAVED!";

        let ciphertext = encrypt(&key, &nonce, plaintext).unwrap();
        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();

        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_passphrase_protection() {
        let vault_key = generate_vault_key();
        let passphrase = "test-passphrase-123";

        let protected = protect_vault_key_with_passphrase(&vault_key, passphrase).unwrap();
        let recovered = recover_vault_key_from_passphrase(&protected, passphrase).unwrap();

        assert_eq!(vault_key, recovered);
    }

    #[test]
    fn test_device_certificate() {
        let account_key = AccountKey::generate();
        let device_key = DeviceKey::generate();

        let cert = DeviceCert::new(device_key.public_key_bytes(), &account_key, None).unwrap();

        cert.verify(&account_key).unwrap();
    }
}
