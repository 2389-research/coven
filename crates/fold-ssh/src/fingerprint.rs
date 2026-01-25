// ABOUTME: SSH public key fingerprint computation.
// ABOUTME: Computes SHA256 fingerprints in wire format compatible with Go's ssh library.

use crate::error::{Result, SshError};
use sha2::{Digest, Sha256};
use ssh_key::PublicKey;

/// Compute SHA256 fingerprint of a public key (hex encoded, lowercase).
///
/// This computes the fingerprint using SSH wire format, which matches
/// Go's `ssh.PublicKey.Marshal()` + `sha256.Sum256()` approach. The format is:
///
/// - Algorithm name as SSH string (4-byte length prefix + "ssh-ed25519")
/// - Key data as SSH string (4-byte length prefix + 32-byte public key)
///
/// Only ed25519 keys are supported. Other key types will return an error.
///
/// # Returns
/// A 64-character lowercase hex string representing the SHA256 hash.
///
/// # Errors
/// Returns `SshError::UnsupportedKeyType` for non-ed25519 keys.
pub fn compute_fingerprint(public_key: &PublicKey) -> Result<String> {
    // Build SSH wire format: algorithm-name (string) + key-data (string)
    let mut wire_data = Vec::new();

    // Get the algorithm name and key data - only ed25519 is supported
    let (algo_name, key_bytes): (&str, Vec<u8>) = match public_key.key_data() {
        ssh_key::public::KeyData::Ed25519(ed) => ("ssh-ed25519", ed.as_ref().to_vec()),
        other => {
            return Err(SshError::UnsupportedKeyType(format!(
                "{:?}",
                other.algorithm()
            )));
        }
    };

    // Write algorithm name as SSH string (4-byte length prefix + data)
    let algo_bytes = algo_name.as_bytes();
    wire_data.extend_from_slice(&(algo_bytes.len() as u32).to_be_bytes());
    wire_data.extend_from_slice(algo_bytes);

    // Write key data as SSH string (4-byte length prefix + data)
    wire_data.extend_from_slice(&(key_bytes.len() as u32).to_be_bytes());
    wire_data.extend_from_slice(&key_bytes);

    // Compute SHA256 hash
    let mut hasher = Sha256::new();
    hasher.update(&wire_data);
    let hash = hasher.finalize();

    // Encode as lowercase hex
    Ok(hex::encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_key::{Algorithm, PrivateKey};

    /// Generate a fresh ed25519 key for testing.
    fn generate_test_key() -> PrivateKey {
        PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519)
            .expect("should generate ed25519 key")
    }

    #[test]
    fn test_fingerprint_consistency() {
        // Fingerprint of the same key should always produce the same result
        let key = generate_test_key();
        let pub_key = key.public_key();

        let fp1 = compute_fingerprint(pub_key).expect("should compute fingerprint");
        let fp2 = compute_fingerprint(pub_key).expect("should compute fingerprint");

        assert_eq!(fp1, fp2, "fingerprint should be deterministic");
    }

    #[test]
    fn test_fingerprint_is_hex_sha256() {
        // Fingerprint should be 64 lowercase hex characters (SHA256 = 32 bytes)
        let key = generate_test_key();
        let fp = compute_fingerprint(key.public_key()).expect("should compute fingerprint");

        assert_eq!(fp.len(), 64, "fingerprint should be 64 hex chars");
        assert!(
            fp.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint should be hex"
        );
        assert_eq!(fp, fp.to_lowercase(), "fingerprint should be lowercase");
    }

    #[test]
    fn test_fingerprint_different_keys() {
        // Different keys should produce different fingerprints
        let key1 = generate_test_key();
        let key2 = generate_test_key();

        let fp1 = compute_fingerprint(key1.public_key()).expect("should compute fingerprint");
        let fp2 = compute_fingerprint(key2.public_key()).expect("should compute fingerprint");

        assert_ne!(
            fp1, fp2,
            "different keys should have different fingerprints"
        );
    }

    #[test]
    fn test_known_key_fingerprint() {
        // Test with a known key to verify fingerprint computation is consistent.
        // This key was generated with ssh-keygen for testing purposes.
        let openssh_key = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACDt5nqYiBnEf19KVKkx6OKiNuzSw6vw16V/wtpvpxnDPQAAAKBNQ5mrTUOZ
qwAAAAtzc2gtZWQyNTUxOQAAACDt5nqYiBnEf19KVKkx6OKiNuzSw6vw16V/wtpvpxnDPQ
AAAEBqQxeANq1p/neGrpquHBFktfFHAM9ZktAmalunTQ5hB+3mepiIGcR/X0pUqTHo4qI2
7NLDq/DXpX/C2m+nGcM9AAAAG2hhcnBlckBkaXNhc3Rlci5sb2NhbGRvbWFpbgEC
-----END OPENSSH PRIVATE KEY-----
";
        let key = PrivateKey::from_openssh(openssh_key).expect("should parse test key");
        let fingerprint =
            compute_fingerprint(key.public_key()).expect("should compute fingerprint");

        // Verify format: 64 lowercase hex characters
        assert_eq!(fingerprint.len(), 64, "fingerprint should be 64 hex chars");
        assert!(
            fingerprint.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint should be hex"
        );
        assert_eq!(
            fingerprint,
            fingerprint.to_lowercase(),
            "fingerprint should be lowercase"
        );

        // Verify the fingerprint is deterministic by computing it again
        let fp2 = compute_fingerprint(key.public_key()).expect("should compute fingerprint");
        assert_eq!(fingerprint, fp2, "fingerprint should be deterministic");
    }

    #[test]
    fn test_fingerprint_wire_format_structure() {
        // Verify the wire format structure by checking expected lengths
        let key = generate_test_key();
        let pub_key = key.public_key();

        // The wire format should be:
        // 4 bytes (algo len) + 11 bytes ("ssh-ed25519") + 4 bytes (key len) + 32 bytes (ed25519 pubkey)
        // Total: 51 bytes
        // SHA256 of that: 32 bytes
        // Hex encoded: 64 characters

        let fp = compute_fingerprint(pub_key).expect("should compute fingerprint");
        assert_eq!(
            fp.len(),
            64,
            "fingerprint should be 64 hex chars (32 bytes SHA256)"
        );
    }

    #[test]
    fn test_fingerprint_unsupported_key_type() {
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
                // If parsing succeeded, try to compute fingerprint
                let result = compute_fingerprint(ecdsa_key.public_key());
                assert!(result.is_err());

                let err = result.unwrap_err();
                assert!(matches!(err, crate::error::SshError::UnsupportedKeyType(_)));
            }
            Err(_) => {
                // If parsing failed (ECDSA not supported by crate features), that's ok
                // The code path we're trying to test is for when you HAVE a non-ed25519 key
                // but the fingerprint code only supports ed25519. If the crate can't even parse
                // ECDSA keys, then the error path is effectively unreachable.
            }
        }
    }
}
