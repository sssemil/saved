use libp2p::identity;
use rand::SeedableRng;
use rsa::{RsaPrivateKey, pkcs8::EncodePrivateKey};
use sha2::{Digest, Sha256};

use rand_chacha::ChaCha8Rng;

/// Generate a deterministic RSA keypair from a seed phrase
///
/// This function uses SHA256 to hash the seed phrase and then uses that hash
/// to seed a deterministic random number generator for RSA key generation.
/// This ensures that the same seed phrase will always generate the same keypair.
pub fn keypair_from_seed(seed: &str) -> identity::Keypair {
    // Use SHA256 to derive a seed for deterministic key generation
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let seed_hash = hasher.finalize();

    // Use the hash as a seed for RSA key generation
    // We'll use a simple approach: take the first 32 bytes as seed
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&seed_hash);

    // Create a deterministic RSA private key using seeded RNG
    let mut rng = ChaCha8Rng::from_seed(seed_bytes);
    let private_key =
        RsaPrivateKey::new(&mut rng, 2048).expect("Failed to generate RSA private key");

    // Convert to PKCS#8 format
    let pkcs8_der = private_key
        .to_pkcs8_der()
        .expect("Failed to encode RSA private key");

    // Create libp2p RSA keypair
    let mut pkcs8_bytes = pkcs8_der.to_bytes();
    identity::rsa::Keypair::try_decode_pkcs8(&mut pkcs8_bytes)
        .expect("Failed to create RSA keypair from PKCS#8")
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_key_generation() {
        let seed = "test-seed-123";

        // Generate keypair twice with the same seed
        let keypair1 = keypair_from_seed(seed);
        let keypair2 = keypair_from_seed(seed);

        // Get the PeerIds
        let peer_id1 = keypair1.public().to_peer_id();
        let peer_id2 = keypair2.public().to_peer_id();

        // They should be identical
        assert_eq!(peer_id1, peer_id2, "Same seed should produce same PeerId");
    }

    #[test]
    fn test_different_seeds_produce_different_keys() {
        let seed1 = "seed-1";
        let seed2 = "seed-2";

        let keypair1 = keypair_from_seed(seed1);
        let keypair2 = keypair_from_seed(seed2);

        let peer_id1 = keypair1.public().to_peer_id();
        let peer_id2 = keypair2.public().to_peer_id();

        // They should be different
        assert_ne!(
            peer_id1, peer_id2,
            "Different seeds should produce different PeerIds"
        );
    }

    #[test]
    fn test_known_seed_peer_ids() {
        // Test with the seeds we've been using
        let keypair1 = keypair_from_seed("1");
        let keypair2 = keypair_from_seed("2");

        let peer_id1 = keypair1.public().to_peer_id();
        let peer_id2 = keypair2.public().to_peer_id();

        // Print the PeerIds for reference
        println!("Seed '1' produces PeerId: {}", peer_id1);
        println!("Seed '2' produces PeerId: {}", peer_id2);

        // They should be different
        assert_ne!(
            peer_id1, peer_id2,
            "Seeds '1' and '2' should produce different PeerIds"
        );

        // They should be valid PeerIds (not empty)
        assert!(
            !peer_id1.to_string().is_empty(),
            "PeerId should not be empty"
        );
        assert!(
            !peer_id2.to_string().is_empty(),
            "PeerId should not be empty"
        );
    }

    #[test]
    fn test_empty_seed() {
        let keypair = keypair_from_seed("");
        let peer_id = keypair.public().to_peer_id();

        // Empty seed should still produce a valid keypair
        assert!(
            !peer_id.to_string().is_empty(),
            "Empty seed should produce valid PeerId"
        );
    }

    #[test]
    fn test_long_seed() {
        let long_seed = "this-is-a-very-long-seed-phrase-that-should-still-work-correctly-and-produce-a-deterministic-keypair";
        let keypair1 = keypair_from_seed(long_seed);
        let keypair2 = keypair_from_seed(long_seed);

        let peer_id1 = keypair1.public().to_peer_id();
        let peer_id2 = keypair2.public().to_peer_id();

        // Long seed should still be deterministic
        assert_eq!(peer_id1, peer_id2, "Long seed should produce same PeerId");
    }

    #[test]
    fn test_unicode_seed() {
        let unicode_seed = "测试种子🌱";
        let keypair1 = keypair_from_seed(unicode_seed);
        let keypair2 = keypair_from_seed(unicode_seed);

        let peer_id1 = keypair1.public().to_peer_id();
        let peer_id2 = keypair2.public().to_peer_id();

        // Unicode seed should be deterministic
        assert_eq!(
            peer_id1, peer_id2,
            "Unicode seed should produce same PeerId"
        );
    }
}
