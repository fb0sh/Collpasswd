use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hkdf::Hkdf;
use p256::ecdh::diffie_hellman;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use rand::RngCore;
use sha2::Sha256;
use zeroize::Zeroize;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;


// ECIES encryption using P-256 + AES-256-GCM + HKDF-SHA256
//
// Wire format: [ephemeral_pk_len: 2 bytes big-endian][ephemeral_pk: var][nonce: 12 bytes][ciphertext: var]

const NONCE_SIZE: usize = 12;
const MASTER_KEY_BYTES: usize = 32;

#[derive(Debug, Clone)]
pub struct EccCrypto;

impl EccCrypto {

    /// Generate a new P-256 key pair
    pub fn generate_keypair() -> Result<(SecretKey, PublicKey)> {
        let secret_key = SecretKey::random(&mut OsRng);
        let public_key = secret_key.public_key();
        Ok((secret_key, public_key))
    }

    /// Serialize secret key to PEM bytes
    pub fn secret_key_to_pem(sk: &SecretKey) -> Result<String> {
        use p256::pkcs8::EncodePrivateKey;
        Ok(sk.to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
            .context("Failed to serialize secret key to PEM")?
            .to_string())
    }

    /// Serialize public key to PEM bytes
    pub fn public_key_to_pem(pk: &PublicKey) -> Result<String> {
        use p256::pkcs8::EncodePublicKey;
        Ok(pk.to_public_key_pem(p256::pkcs8::LineEnding::LF)
            .context("Failed to serialize public key to PEM")?)
    }

    /// Deserialize secret key from PEM
    pub fn secret_key_from_pem(pem: &str) -> Result<SecretKey> {
        use p256::pkcs8::DecodePrivateKey;
        Ok(SecretKey::from_pkcs8_pem(pem)
            .context("Failed to deserialize secret key from PEM")?)
    }

    /// Deserialize public key from PEM
    pub fn public_key_from_pem(pem: &str) -> Result<PublicKey> {
        use p256::pkcs8::DecodePublicKey;
        Ok(PublicKey::from_public_key_pem(pem)
            .context("Failed to deserialize public key from PEM")?)
    }

    /// Encrypt plaintext using ECIES with the recipient's public key
    /// Returns base64-encoded ciphertext
    pub fn encrypt(pk: &PublicKey, plaintext: &[u8]) -> Result<String> {
        // Generate ephemeral key pair
        let ephemeral_sk = SecretKey::random(&mut OsRng);
        let ephemeral_pk = ephemeral_sk.public_key();

        // ECDH: shared secret = ephemeral_sk * pk
        let shared_secret = diffie_hellman(
            &ephemeral_sk.to_nonzero_scalar(),
            pk.as_affine(),
        );
        let shared_bytes: &[u8] = shared_secret.raw_secret_bytes();

        // Derive AES-256 key using HKDF
        let hkdf = Hkdf::<Sha256>::new(None, shared_bytes);
        let mut aes_key = [0u8; 32];
        hkdf.expand(b"ecc-aes-gcm-key", &mut aes_key)
            .context("HKDF expand failed")?;

        // Encrypt with AES-256-GCM
        let cipher = Aes256Gcm::new_from_slice(&aes_key)
            .context("Failed to create AES cipher")?;

        let mut nonce_bytes = [0u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("AES encryption failed: {:?}", e))?;

        // Serialize ephemeral public key (compressed form, 33 bytes for P-256)
        let epk_encoded = ephemeral_pk.to_encoded_point(true);
        let epk_bytes = epk_encoded.as_bytes().to_vec();

        // Format: [epk_len:2][epk_bytes:var][nonce:12][ciphertext:var]
        let mut result = Vec::with_capacity(2 + epk_bytes.len() + NONCE_SIZE + ciphertext.len());
        result.extend_from_slice(&(epk_bytes.len() as u16).to_be_bytes());
        result.extend_from_slice(&epk_bytes);
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);

        Ok(BASE64.encode(&result))
    }

    /// Decrypt base64-encoded ciphertext using the recipient's secret key
    pub fn decrypt(sk: &SecretKey, encrypted_b64: &str) -> Result<Vec<u8>> {
        let data = BASE64
            .decode(encrypted_b64)
            .context("Failed to decode base64 ciphertext")?;

        if data.len() < 2 + 33 + NONCE_SIZE {
            anyhow::bail!("Ciphertext too short");
        }

        let mut offset = 0;

        // Read ephemeral public key length
        let epk_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;

        if data.len() < offset + epk_len + NONCE_SIZE {
            anyhow::bail!("Ciphertext too short for EPK");
        }

        // Extract ephemeral public key
        let epk_bytes = &data[offset..offset + epk_len];
        offset += epk_len;

        // Extract nonce
        let nonce_bytes = &data[offset..offset + NONCE_SIZE];
        let nonce = Nonce::from_slice(nonce_bytes);
        offset += NONCE_SIZE;

        // Extract ciphertext
        let ciphertext = &data[offset..];

        // Reconstruct ephemeral public key
        let ephemeral_pk = PublicKey::from_sec1_bytes(epk_bytes)
            .context("Failed to parse ephemeral public key")?;

        // ECDH: shared secret = sk * ephemeral_pk
        let shared_secret = diffie_hellman(
            &sk.to_nonzero_scalar(),
            ephemeral_pk.as_affine(),
        );
        let shared_bytes: &[u8] = shared_secret.raw_secret_bytes();

        // Derive the same AES-256 key
        let hkdf = Hkdf::<Sha256>::new(None, shared_bytes);
        let mut aes_key = [0u8; 32];
        hkdf.expand(b"ecc-aes-gcm-key", &mut aes_key)
            .context("HKDF expand failed")?;

        // Decrypt
        let cipher = Aes256Gcm::new_from_slice(&aes_key)
            .context("Failed to create AES cipher")?;

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("AES decryption failed: {:?}", e))?;

        Ok(plaintext)
    }
}

// ─── Master Key ──────────────────────────────────────────────────────────────
//
// The master key is DERIVED from the admin's login password using HKDF-SHA256 +
// a random salt stored in the DB. The key NEVER touches disk — it lives only
// in server memory. Losing the password means permanent data loss (by design).
//
// For headless/automated deployments, MasterKey::from_file() provides a
// fallback using a separate key file with 0600 permissions.

/// Wraps a 256-bit master key used to encrypt/decrypt book ECC private keys.
/// Automatically zeroes on drop.
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct MasterKey([u8; MASTER_KEY_BYTES]);

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterKey").finish_non_exhaustive()
    }
}

impl MasterKey {
    /// Derive a 256-bit key from a password + random salt using HKDF-SHA256.
    ///
    /// The salt is stored in the DB (config table). The password is the admin's
    /// login password — never stored, only remembered by the human operator.
    pub fn derive_from_password(password: &str, salt: &[u8]) -> Self {
        let hkdf = Hkdf::<Sha256>::new(Some(salt), password.as_bytes());
        let mut key = [0u8; MASTER_KEY_BYTES];
        hkdf.expand(b"collpasswd-master-key", &mut key)
            .expect("HKDF expand should never fail with valid output length");
        MasterKey(key)
    }

    /// Generate a random salt for key derivation (16 bytes is sufficient).
    pub fn generate_salt() -> [u8; 16] {
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        salt
    }

    /// Load master key from a key file (fallback for automated deployments).
    pub fn from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!("Master key file not found: {:?}", path);
        }
        let mut data = String::new();
        fs::File::open(path)
            .context("Failed to open master key file")?
            .read_to_string(&mut data)
            .context("Failed to read master key file")?;
        let data = data.trim();
        let decoded = BASE64
            .decode(data)
            .context("Master key file: invalid base64")?;
        if decoded.len() != MASTER_KEY_BYTES {
            anyhow::bail!(
                "Master key file: expected {} bytes, got {}",
                MASTER_KEY_BYTES,
                decoded.len()
            );
        }
        let mut key = [0u8; MASTER_KEY_BYTES];
        key.copy_from_slice(&decoded);
        Ok(MasterKey(key))
    }

    /// Encrypt a PEM-encoded ECC private key. Returns base64.
    pub fn encrypt_private_key(&self, pem: &str) -> Result<String> {
        let cipher =
            Aes256Gcm::new_from_slice(&self.0).context("Failed to create AES cipher")?;

        let mut nonce_bytes = [0u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, pem.as_bytes())
            .map_err(|e| anyhow::anyhow!("Master key encrypt failed: {:?}", e))?;

        let mut out = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(BASE64.encode(&out))
    }

    /// Decrypt a book's ECC private key. Returns PEM string.
    pub fn decrypt_private_key(&self, encrypted_b64: &str) -> Result<String> {
        let data = BASE64
            .decode(encrypted_b64)
            .context("Failed to decode encrypted private key")?;

        if data.len() < NONCE_SIZE + 1 {
            anyhow::bail!("Encrypted private key too short");
        }

        let nonce = Nonce::from_slice(&data[..NONCE_SIZE]);
        let ciphertext = &data[NONCE_SIZE..];

        let cipher =
            Aes256Gcm::new_from_slice(&self.0).context("Failed to create AES cipher")?;

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Master key decrypt failed: {:?}", e))?;

        String::from_utf8(plaintext).context("Decrypted private key is not valid UTF-8")
    }
}

impl fmt::Display for EccCrypto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EccCrypto(P-256)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ecc_encrypt_decrypt() {
        let (sk, pk) = EccCrypto::generate_keypair().unwrap();
        let plaintext = b"Hello, World! This is a secret password: s3cur3!";

        let encrypted = EccCrypto::encrypt(&pk, plaintext).unwrap();
        assert_ne!(encrypted, BASE64.encode(plaintext));

        let decrypted = EccCrypto::decrypt(&sk, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_key_serialization() {
        let (sk, pk) = EccCrypto::generate_keypair().unwrap();

        let sk_pem = EccCrypto::secret_key_to_pem(&sk).unwrap();
        let pk_pem = EccCrypto::public_key_to_pem(&pk).unwrap();

        let sk2 = EccCrypto::secret_key_from_pem(&sk_pem).unwrap();
        let pk2 = EccCrypto::public_key_from_pem(&pk_pem).unwrap();

        let plaintext = b"test key serialization";
        let encrypted = EccCrypto::encrypt(&pk2, plaintext).unwrap();
        let decrypted = EccCrypto::decrypt(&sk2, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let (sk1, pk1) = EccCrypto::generate_keypair().unwrap();
        let (_sk2, _pk2) = EccCrypto::generate_keypair().unwrap();

        let plaintext = b"secret data";
        let encrypted = EccCrypto::encrypt(&pk1, plaintext).unwrap();

        assert!(EccCrypto::decrypt(&_sk2, &encrypted).is_err());
    }
}
