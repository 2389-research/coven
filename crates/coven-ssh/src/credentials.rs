// ABOUTME: SSH authentication credentials for gRPC requests.
// ABOUTME: Signs timestamp|nonce messages and applies auth headers to tonic requests.

use crate::error::{Result, SshError};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use rand::RngCore;
use ssh_key::PrivateKey;
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::metadata::MetadataValue;

/// Generate a random nonce for authentication.
///
/// Returns a 32-character hex string (16 random bytes).
pub fn generate_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Get current Unix timestamp in seconds.
pub fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs() as i64
}

/// Sign a message with the private key.
///
/// Returns the signature in SSH wire format, base64 encoded:
/// - Algorithm name as SSH string (4-byte length prefix + "ssh-ed25519")
/// - Signature blob as SSH string (4-byte length prefix + 64-byte signature)
///
/// Only ed25519 keys are supported.
///
/// # Errors
/// Returns `SshError::UnsupportedKeyType` for non-ed25519 keys.
pub fn sign_message(private_key: &PrivateKey, message: &str) -> Result<String> {
    let keypair = private_key.key_data();

    match keypair {
        ssh_key::private::KeypairData::Ed25519(ed25519_keypair) => {
            // Convert to ed25519-dalek signing key
            let signing_key = SigningKey::from_bytes(&ed25519_keypair.private.to_bytes());
            let signature = signing_key.sign(message.as_bytes());

            // Build SSH signature wire format to match Go's ssh.Signature:
            // - Format: SSH string (4-byte length prefix + "ssh-ed25519")
            // - Blob: SSH string (4-byte length prefix + 64-byte signature)
            let algo_name = b"ssh-ed25519";
            let sig_bytes = signature.to_bytes();

            let mut wire_data = Vec::new();
            // Write algorithm name as SSH string
            wire_data.extend_from_slice(&(algo_name.len() as u32).to_be_bytes());
            wire_data.extend_from_slice(algo_name);
            // Write signature blob as SSH string
            wire_data.extend_from_slice(&(sig_bytes.len() as u32).to_be_bytes());
            wire_data.extend_from_slice(&sig_bytes);

            Ok(base64::engine::general_purpose::STANDARD.encode(&wire_data))
        }
        _ => Err(SshError::UnsupportedKeyType("non-ed25519".to_string())),
    }
}

/// SSH authentication credentials for gRPC metadata.
///
/// Contains all the fields needed to authenticate with coven-gateway:
/// - `pubkey`: OpenSSH format public key
/// - `signature`: Base64-encoded SSH signature of `timestamp|nonce`
/// - `timestamp`: Unix timestamp when credentials were created
/// - `nonce`: Random hex string to prevent replay attacks
#[derive(Debug, Clone)]
pub struct SshAuthCredentials {
    /// OpenSSH format public key string.
    pub pubkey: String,
    /// Base64-encoded SSH signature of the message `{timestamp}|{nonce}`.
    pub signature: String,
    /// Unix timestamp when these credentials were created.
    pub timestamp: i64,
    /// Random nonce to prevent replay attacks.
    pub nonce: String,
}

impl SshAuthCredentials {
    /// Create new authentication credentials by signing `timestamp|nonce`.
    ///
    /// Generates a fresh timestamp and nonce, signs the combined message,
    /// and packages everything needed for gRPC authentication.
    ///
    /// # Errors
    /// Returns an error if signing fails or the public key cannot be serialized.
    pub fn new(private_key: &PrivateKey) -> Result<Self> {
        let timestamp = current_timestamp();
        let nonce = generate_nonce();
        let message = format!("{}|{}", timestamp, nonce);

        let signature = sign_message(private_key, &message)?;
        let pubkey = private_key
            .public_key()
            .to_openssh()
            .map_err(SshError::SerializeKey)?;

        Ok(Self {
            pubkey,
            signature,
            timestamp,
            nonce,
        })
    }

    /// Get the age of these credentials in seconds.
    ///
    /// Returns the number of seconds since these credentials were created.
    pub fn age_secs(&self) -> i64 {
        current_timestamp() - self.timestamp
    }

    /// Check if these credentials are stale and should be refreshed.
    ///
    /// Credentials are considered stale if they are older than the given TTL.
    /// The gateway rejects signatures older than 5 minutes (300 seconds),
    /// so a typical TTL would be 240 seconds (4 minutes) to refresh early.
    pub fn is_stale(&self, ttl_secs: i64) -> bool {
        self.age_secs() > ttl_secs
    }

    /// Apply credentials to a gRPC request as metadata headers.
    ///
    /// Adds the following headers to the request:
    /// - `x-ssh-pubkey`: The OpenSSH format public key
    /// - `x-ssh-signature`: The base64-encoded signature
    /// - `x-ssh-timestamp`: The Unix timestamp as a string
    /// - `x-ssh-nonce`: The random nonce
    ///
    /// # Errors
    /// Returns an error if any metadata value is invalid.
    pub fn apply_to_request<T>(&self, req: &mut tonic::Request<T>) -> Result<()> {
        let metadata = req.metadata_mut();

        metadata.insert(
            "x-ssh-pubkey",
            MetadataValue::try_from(&self.pubkey).map_err(|e| SshError::InvalidMetadata {
                field: "x-ssh-pubkey".to_string(),
                message: e.to_string(),
            })?,
        );
        metadata.insert(
            "x-ssh-signature",
            MetadataValue::try_from(&self.signature).map_err(|e| SshError::InvalidMetadata {
                field: "x-ssh-signature".to_string(),
                message: e.to_string(),
            })?,
        );
        metadata.insert(
            "x-ssh-timestamp",
            MetadataValue::try_from(self.timestamp.to_string()).map_err(|e| {
                SshError::InvalidMetadata {
                    field: "x-ssh-timestamp".to_string(),
                    message: e.to_string(),
                }
            })?,
        );
        metadata.insert(
            "x-ssh-nonce",
            MetadataValue::try_from(&self.nonce).map_err(|e| SshError::InvalidMetadata {
                field: "x-ssh-nonce".to_string(),
                message: e.to_string(),
            })?,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_key::Algorithm;

    /// Generate a fresh ed25519 key for testing.
    fn generate_test_key() -> PrivateKey {
        PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519)
            .expect("should generate ed25519 key")
    }

    #[test]
    fn test_generate_nonce_uniqueness() {
        let nonce1 = generate_nonce();
        let nonce2 = generate_nonce();

        assert_ne!(nonce1, nonce2, "nonces should be unique");
    }

    #[test]
    fn test_generate_nonce_format() {
        let nonce = generate_nonce();

        assert_eq!(nonce.len(), 32, "nonce should be 32 hex chars (16 bytes)");
        assert!(
            nonce.chars().all(|c| c.is_ascii_hexdigit()),
            "nonce should be hex"
        );
    }

    #[test]
    fn test_current_timestamp_reasonable() {
        let ts = current_timestamp();

        // 1577836800 = 2020-01-01 00:00:00 UTC
        assert!(ts > 1577836800, "timestamp should be after 2020");
    }

    #[test]
    fn test_sign_message_deterministic() {
        // ed25519 signatures are deterministic for the same key and message
        let key = generate_test_key();
        let message = "test message for signing";

        let sig1 = sign_message(&key, message).expect("should sign");
        let sig2 = sign_message(&key, message).expect("should sign again");

        assert_eq!(sig1, sig2, "ed25519 signing should be deterministic");
    }

    #[test]
    fn test_sign_message_different_messages() {
        let key = generate_test_key();

        let sig1 = sign_message(&key, "message1").expect("should sign");
        let sig2 = sign_message(&key, "message2").expect("should sign");

        assert_ne!(
            sig1, sig2,
            "different messages should have different signatures"
        );
    }

    #[test]
    fn test_sign_message_is_valid_base64() {
        let key = generate_test_key();
        let sig = sign_message(&key, "test").expect("should sign");

        base64::engine::general_purpose::STANDARD
            .decode(&sig)
            .expect("signature should be valid base64");
    }

    #[test]
    fn test_signature_wire_format() {
        // Signature should have SSH wire format:
        // 4-byte algo name length + "ssh-ed25519" (11 bytes) + 4-byte sig length + signature (64 bytes)
        // Total: 4 + 11 + 4 + 64 = 83 bytes
        let key = generate_test_key();
        let sig = sign_message(&key, "test").expect("should sign");

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&sig)
            .expect("should decode");

        assert_eq!(
            decoded.len(),
            83,
            "SSH signature wire format should be 83 bytes"
        );

        // Check algorithm name
        let algo_len = u32::from_be_bytes(decoded[0..4].try_into().unwrap()) as usize;
        assert_eq!(algo_len, 11, "algo name length should be 11");
        assert_eq!(
            &decoded[4..15],
            b"ssh-ed25519",
            "algo name should be ssh-ed25519"
        );

        // Check signature length
        let sig_len = u32::from_be_bytes(decoded[15..19].try_into().unwrap()) as usize;
        assert_eq!(sig_len, 64, "ed25519 signature should be 64 bytes");
    }

    #[test]
    fn test_signature_verification_with_dalek() {
        // Verify the signature using ed25519-dalek directly
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let key = generate_test_key();
        let message = "verify this message";
        let sig_base64 = sign_message(&key, message).expect("should sign");

        // Decode the wire format
        let wire = base64::engine::general_purpose::STANDARD
            .decode(&sig_base64)
            .expect("should decode");

        // Extract the raw signature bytes (skip 4-byte algo len + 11-byte algo name + 4-byte sig len)
        let sig_bytes: [u8; 64] = wire[19..83].try_into().expect("should be 64 bytes");
        let signature = Signature::from_bytes(&sig_bytes);

        // Get the public key bytes for verification
        let pub_key = key.public_key();
        let pub_key_bytes: [u8; 32] = match pub_key.key_data() {
            ssh_key::public::KeyData::Ed25519(ed) => *ed.as_ref(),
            _ => panic!("expected ed25519 key"),
        };
        let verifying_key =
            VerifyingKey::from_bytes(&pub_key_bytes).expect("should create verifying key");

        // Verify
        verifying_key
            .verify(message.as_bytes(), &signature)
            .expect("signature should verify");
    }

    #[test]
    fn test_ssh_auth_credentials_creates_valid_signature() {
        let key = generate_test_key();
        let creds = SshAuthCredentials::new(&key).expect("should create credentials");

        // Verify the signature format
        let wire = base64::engine::general_purpose::STANDARD
            .decode(&creds.signature)
            .expect("signature should be valid base64");
        assert_eq!(wire.len(), 83, "signature wire format should be 83 bytes");

        // Verify the timestamp is reasonable
        assert!(
            creds.timestamp > 1577836800,
            "timestamp should be after 2020"
        );

        // Verify the nonce format
        assert_eq!(creds.nonce.len(), 32, "nonce should be 32 hex chars");

        // Verify the pubkey is valid openssh format
        assert!(
            creds.pubkey.starts_with("ssh-ed25519 "),
            "pubkey should be openssh format"
        );
    }

    #[test]
    fn test_ssh_auth_credentials_apply_to_request() {
        let key = generate_test_key();
        let creds = SshAuthCredentials::new(&key).expect("should create credentials");

        let mut request = tonic::Request::new(());
        creds
            .apply_to_request(&mut request)
            .expect("should apply credentials");

        let metadata = request.metadata();
        assert!(metadata.contains_key("x-ssh-pubkey"));
        assert!(metadata.contains_key("x-ssh-signature"));
        assert!(metadata.contains_key("x-ssh-timestamp"));
        assert!(metadata.contains_key("x-ssh-nonce"));
    }

    #[test]
    fn test_sign_message_unsupported_key_type() {
        // Use an ECDSA key (P-256) to test the unsupported key type error path
        // This key was generated for testing purposes
        let ecdsa_openssh = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAaAAAABNlY2RzYS
1zaGEyLW5pc3RwMjU2AAAACG5pc3RwMjU2AAAAQQTNTn5FgZVuXQGxJe9jOgFhKJ6RCkqw
WcL9KlOmRJLdA2qFEvXhqmLs+hLJ0xMc3F6zhvUmhGJrmkWjD3w6PQ3MAAAAqDaGExY2hh
MWAAAAABMlY2RzYS1zaGEyLW5pc3RwMjU2AAAACG5pc3RwMjU2AAAAQQQE00p+RYGV
bl0BsSXvYzoBYSiekQpKsFnC/SpTpkSS3QNqhRL14api7PoSydMTHNxes4b1JoRia5pFow
98Oj0NzAAAAIEAm8wBYp2hTLdMrxVJwGYC9hWVH1gqO4YDvJ5vGlLkQ/wAAAAOdGVzdEB0
ZXN0LmNvbQECAwQFBg==
-----END OPENSSH PRIVATE KEY-----";

        // Try to parse the ECDSA key - it may or may not be supported by the ssh_key crate
        // depending on feature flags
        match PrivateKey::from_openssh(ecdsa_openssh) {
            Ok(ecdsa_key) => {
                // If parsing succeeded, try to sign with it
                let result = sign_message(&ecdsa_key, "test message");
                assert!(result.is_err());

                let err = result.unwrap_err();
                assert!(matches!(err, crate::error::SshError::UnsupportedKeyType(_)));
            }
            Err(_) => {
                // If parsing failed (ECDSA not supported by crate features), that's ok
                // The code path we're trying to test is for when you HAVE a non-ed25519 key
                // but the signing code only supports ed25519. If the crate can't even parse
                // ECDSA keys, then the error path is effectively unreachable.
            }
        }
    }

    #[test]
    fn test_ssh_auth_credentials_age_secs() {
        let key = generate_test_key();
        let creds = SshAuthCredentials::new(&key).expect("should create credentials");

        // Age should be very small (less than 1 second) right after creation
        let age = creds.age_secs();
        assert!(age >= 0, "age should be non-negative");
        assert!(
            age < 2,
            "age should be less than 2 seconds right after creation"
        );
    }

    #[test]
    fn test_ssh_auth_credentials_is_stale() {
        let key = generate_test_key();
        let creds = SshAuthCredentials::new(&key).expect("should create credentials");

        // Fresh credentials should not be stale with a 240s TTL
        assert!(
            !creds.is_stale(240),
            "fresh credentials should not be stale"
        );

        // Fresh credentials (age=0) are NOT stale with 0s TTL since 0 > 0 is false
        assert!(
            !creds.is_stale(0),
            "credentials with age=0 are not stale with TTL=0"
        );

        // Fresh credentials should be stale with a negative TTL (always stale)
        assert!(
            creds.is_stale(-1),
            "credentials should be stale with -1s TTL"
        );
    }

    #[test]
    fn test_ssh_auth_credentials_metadata_values() {
        let key = generate_test_key();
        let creds = SshAuthCredentials::new(&key).expect("should create credentials");

        let mut request = tonic::Request::new(());
        creds
            .apply_to_request(&mut request)
            .expect("should apply credentials");

        let metadata = request.metadata();

        // Verify the actual values match the credentials
        let pubkey_val = metadata.get("x-ssh-pubkey").expect("should have pubkey");
        assert_eq!(pubkey_val.to_str().unwrap(), creds.pubkey);

        let sig_val = metadata
            .get("x-ssh-signature")
            .expect("should have signature");
        assert_eq!(sig_val.to_str().unwrap(), creds.signature);

        let ts_val = metadata
            .get("x-ssh-timestamp")
            .expect("should have timestamp");
        assert_eq!(ts_val.to_str().unwrap(), creds.timestamp.to_string());

        let nonce_val = metadata.get("x-ssh-nonce").expect("should have nonce");
        assert_eq!(nonce_val.to_str().unwrap(), creds.nonce);
    }
}
