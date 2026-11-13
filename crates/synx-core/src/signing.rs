//! Ed25519 package signing and verification for SYNX marker packages.
//!
//! Each package tarball is hashed with SHA-256, then signed with an Ed25519 key.
//! The signature is stored alongside the package as `<name>.sig`.

use ed25519_dalek::Signer;
use sha2::{Digest, Sha256};
use std::path::Path;

/// A signing keypair for package authors.
pub struct SigningKey {
    keypair: ed25519_dalek::SigningKey,
}

/// A public key for verifying package signatures.
pub struct VerifyKey {
    pubkey: ed25519_dalek::VerifyingKey,
}

/// A detached signature over a file.
#[derive(Clone)]
pub struct PackageSignature {
    pub signature: Vec<u8>,
    pub public_key: Vec<u8>,
}

impl SigningKey {
    /// Generate a new random keypair.
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let keypair = ed25519_dalek::SigningKey::generate(&mut rng);
        SigningKey { keypair }
    }

    /// Load from raw 32-byte secret key.
    pub fn from_bytes(secret: &[u8; 32]) -> Self {
        let keypair = ed25519_dalek::SigningKey::from_bytes(secret);
        SigningKey { keypair }
    }

    /// Load from hex-encoded 32-byte secret key.
    pub fn from_hex(hex: &str) -> Result<Self, String> {
        let bytes = hex_decode(hex)?;
        if bytes.len() != 32 {
            return Err(format!("expected 32 bytes, got {}", bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self::from_bytes(&arr))
    }

    /// Export secret key as hex string.
    pub fn to_hex(&self) -> String {
        hex_encode(&self.keypair.to_bytes())
    }

    /// Export the secret key bytes (32 bytes).
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.keypair.to_bytes()
    }

    /// Export the public key bytes (32 bytes).
    pub fn public_bytes(&self) -> [u8; 32] {
        self.keypair.verifying_key().to_bytes()
    }

    /// Get the verify key.
    pub fn verify_key(&self) -> VerifyKey {
        VerifyKey {
            pubkey: self.keypair.verifying_key(),
        }
    }

    /// Sign raw data, return signature bytes.
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        let hash = sha256(data);
        let sig = self.keypair.sign(&hash);
        sig.to_bytes().to_vec()
    }

    /// Sign a file on disk.
    pub fn sign_file(&self, path: &Path) -> Result<PackageSignature, String> {
        let data = std::fs::read(path).map_err(|e| format!("read error: {}", e))?;
        let signature = self.sign(&data);
        Ok(PackageSignature {
            signature,
            public_key: self.public_bytes().to_vec(),
        })
    }
}

impl VerifyKey {
    /// Load from raw 32-byte public key.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, String> {
        let pubkey = ed25519_dalek::VerifyingKey::from_bytes(bytes)
            .map_err(|e| format!("invalid public key: {}", e))?;
        Ok(VerifyKey { pubkey })
    }

    /// Load from hex-encoded 32-byte public key.
    pub fn from_hex(hex: &str) -> Result<Self, String> {
        let bytes = hex_decode(hex)?;
        if bytes.len() != 32 {
            return Err(format!("expected 32 bytes, got {}", bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Self::from_bytes(&arr)
    }

    /// Export public key as hex string.
    pub fn to_hex(&self) -> String {
        hex_encode(&self.pubkey.to_bytes())
    }

    /// Export the public key bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.pubkey.to_bytes()
    }

    /// Verify a signature over raw data.
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<bool, String> {
        let hash = sha256(data);
        let sig = ed25519_dalek::Signature::from_slice(signature)
            .map_err(|e| format!("invalid signature: {}", e))?;
        // safety: strict verification. The plain one accepts keys and nonces
        // of small order: with the identity point as the key and a zero
        // scalar, one fixed 96-byte "signature" verified for every message.
        Ok(self.pubkey.verify_strict(&hash, &sig).is_ok())
    }

    /// Verify a file signature.
    pub fn verify_file(&self, path: &Path, sig: &PackageSignature) -> Result<bool, String> {
        let data = std::fs::read(path).map_err(|e| format!("read error: {}", e))?;
        self.verify(&data, &sig.signature)
    }
}

impl PackageSignature {
    /// Serialize to hex string: "signature_hex:pubkey_hex"
    pub fn to_hex(&self) -> String {
        format!("{}:{}", hex_encode(&self.signature), hex_encode(&self.public_key))
    }

    /// Deserialize from hex string: "signature_hex:pubkey_hex"
    pub fn from_hex(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err("invalid signature format, expected 'sig:pubkey'".to_string());
        }
        let signature = hex_decode(parts[0])?;
        let public_key = hex_decode(parts[1])?;
        Ok(PackageSignature {
            signature,
            public_key,
        })
    }

    /// Verify this signature against data.
    pub fn verify(&self, data: &[u8]) -> Result<bool, String> {
        if self.public_key.len() != 32 {
            return Err("invalid public key length".to_string());
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&self.public_key);
        let vk = VerifyKey::from_bytes(&key_bytes)?;
        vk.verify(data, &self.signature)
    }
}

/// SHA-256 hash of data.
fn sha256(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Decode a string of hex digit pairs.
///
/// safety: by byte, digits only. Slicing the text two bytes at a time
/// panicked inside a multi-byte character (`"aé1"`), and `from_str_radix`
/// took a sign, so `+1` was a byte and one key had many spellings.
fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err("odd-length hex string".to_string());
    }
    bytes
        .chunks_exact(2)
        .enumerate()
        .map(|(i, pair)| match (nibble(pair[0]), nibble(pair[1])) {
            (Some(hi), Some(lo)) => Ok(hi << 4 | lo),
            _ => Err(format!("invalid hex at position {}", i * 2)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keygen_and_roundtrip() {
        let key = SigningKey::generate();
        let hex = key.to_hex();
        let restored = SigningKey::from_hex(&hex).unwrap();
        assert_eq!(key.secret_bytes(), restored.secret_bytes());
    }

    #[test]
    fn test_sign_and_verify() {
        let key = SigningKey::generate();
        let data = b"hello, synx packages";
        let signature = key.sign(data);
        let vk = key.verify_key();
        assert!(vk.verify(data, &signature).unwrap());
    }

    #[test]
    fn test_verify_wrong_data() {
        let key = SigningKey::generate();
        let signature = key.sign(b"correct data");
        let vk = key.verify_key();
        assert!(!vk.verify(b"wrong data", &signature).unwrap());
    }

    #[test]
    fn test_verify_wrong_key() {
        let key1 = SigningKey::generate();
        let key2 = SigningKey::generate();
        let signature = key1.sign(b"data");
        let vk2 = key2.verify_key();
        assert!(!vk2.verify(b"data", &signature).unwrap());
    }

    #[test]
    fn test_verify_key_hex_roundtrip() {
        let key = SigningKey::generate();
        let vk = key.verify_key();
        let hex = vk.to_hex();
        let restored = VerifyKey::from_hex(&hex).unwrap();
        assert_eq!(vk.to_bytes(), restored.to_bytes());
    }

    #[test]
    fn test_package_signature_hex_roundtrip() {
        let key = SigningKey::generate();
        let sig_bytes = key.sign(b"test package data");
        let ps = PackageSignature {
            signature: sig_bytes,
            public_key: key.public_bytes().to_vec(),
        };
        let hex = ps.to_hex();
        let restored = PackageSignature::from_hex(&hex).unwrap();
        assert_eq!(ps.signature, restored.signature);
        assert_eq!(ps.public_key, restored.public_key);
    }

    #[test]
    fn test_package_signature_verify() {
        let key = SigningKey::generate();
        let data = b"package tarball contents";
        let sig_bytes = key.sign(data);
        let ps = PackageSignature {
            signature: sig_bytes,
            public_key: key.public_bytes().to_vec(),
        };
        assert!(ps.verify(data).unwrap());
        assert!(!ps.verify(b"tampered").unwrap());
    }

    #[test]
    fn test_hex_encode_decode() {
        let original = vec![0xde, 0xad, 0xbe, 0xef];
        let encoded = hex_encode(&original);
        assert_eq!(encoded, "deadbeef");
        let decoded = hex_decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_invalid_hex() {
        assert!(hex_decode("xyz").is_err());
        assert!(hex_decode("a").is_err()); // odd length
    }

    // Neither panics nor reads a sign as a digit.
    #[test]
    fn test_hex_is_digits_only() {
        assert!(hex_decode("a\u{e9}1").is_err());
        assert!(hex_decode("\u{e9}\u{e9}").is_err());
        assert!(hex_decode("+1").is_err());
        assert!(hex_decode("-1").is_err());
        assert!(VerifyKey::from_hex("a\u{e9}b").is_err());
        assert!(VerifyKey::from_hex(&"+1".repeat(32)).is_err());
        assert!(PackageSignature::from_hex("a\u{e9}b:00").is_err());
        assert_eq!(hex_decode("00fFAa").unwrap(), vec![0x00, 0xff, 0xaa]);
    }

    // The identity point as the key and a zero scalar made one signature
    // that passed for any data.
    #[test]
    fn test_degenerate_signature_is_refused() {
        let mut sig = vec![0u8; 64];
        sig[0] = 1;
        let mut key = vec![0u8; 32];
        key[0] = 1;
        let forged = PackageSignature { signature: sig, public_key: key };
        assert!(!forged.verify(b"any data").unwrap_or(false));
        assert!(!forged.verify(b"other data").unwrap_or(false));
    }
}
