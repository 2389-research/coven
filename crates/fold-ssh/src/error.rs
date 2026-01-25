// ABOUTME: Error types for SSH key operations using thiserror.
// ABOUTME: Provides typed errors for key loading, generation, signing, and auth.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during SSH key operations.
#[derive(Error, Debug)]
pub enum SshError {
    /// Failed to read a key file from disk.
    #[error("failed to read SSH key from {path}: {source}")]
    ReadKey {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse an SSH key.
    #[error("failed to parse SSH key from {path}: {source}")]
    ParseKey {
        path: PathBuf,
        #[source]
        source: ssh_key::Error,
    },

    /// Failed to generate an SSH key.
    #[error("failed to generate SSH key: {0}")]
    GenerateKey(#[source] ssh_key::Error),

    /// Failed to serialize a key.
    #[error("failed to serialize key: {0}")]
    SerializeKey(#[source] ssh_key::Error),

    /// Failed to write a key file to disk.
    #[error("failed to write key to {path}: {source}")]
    WriteKey {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to create a directory.
    #[error("failed to create directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to set file permissions.
    #[error("failed to set permissions on {path}: {source}")]
    SetPermissions {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Unsupported key type for the requested operation.
    #[error("unsupported key type: {0} (only ed25519 is supported)")]
    UnsupportedKeyType(String),

    /// Failed to add metadata to gRPC request.
    #[error("invalid metadata value for {field}: {message}")]
    InvalidMetadata { field: String, message: String },
}

/// Result type alias using SshError.
pub type Result<T> = std::result::Result<T, SshError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_read_key_error_display() {
        let err = SshError::ReadKey {
            path: PathBuf::from("/path/to/key"),
            source: io::Error::new(io::ErrorKind::NotFound, "file not found"),
        };
        let display = format!("{}", err);
        assert!(display.contains("failed to read SSH key"));
        assert!(display.contains("/path/to/key"));
    }

    #[test]
    fn test_parse_key_error_display() {
        // Create a mock ssh_key::Error by creating a parse error
        let err = SshError::ParseKey {
            path: PathBuf::from("/path/to/invalid_key"),
            source: ssh_key::Error::AlgorithmUnknown,
        };
        let display = format!("{}", err);
        assert!(display.contains("failed to parse SSH key"));
        assert!(display.contains("/path/to/invalid_key"));
    }

    #[test]
    fn test_generate_key_error_display() {
        let err = SshError::GenerateKey(ssh_key::Error::AlgorithmUnknown);
        let display = format!("{}", err);
        assert!(display.contains("failed to generate SSH key"));
    }

    #[test]
    fn test_serialize_key_error_display() {
        let err = SshError::SerializeKey(ssh_key::Error::AlgorithmUnknown);
        let display = format!("{}", err);
        assert!(display.contains("failed to serialize key"));
    }

    #[test]
    fn test_write_key_error_display() {
        let err = SshError::WriteKey {
            path: PathBuf::from("/path/to/key"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
        };
        let display = format!("{}", err);
        assert!(display.contains("failed to write key"));
        assert!(display.contains("/path/to/key"));
    }

    #[test]
    fn test_create_directory_error_display() {
        let err = SshError::CreateDirectory {
            path: PathBuf::from("/path/to/dir"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
        };
        let display = format!("{}", err);
        assert!(display.contains("failed to create directory"));
        assert!(display.contains("/path/to/dir"));
    }

    #[test]
    fn test_set_permissions_error_display() {
        let err = SshError::SetPermissions {
            path: PathBuf::from("/path/to/key"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
        };
        let display = format!("{}", err);
        assert!(display.contains("failed to set permissions"));
        assert!(display.contains("/path/to/key"));
    }

    #[test]
    fn test_unsupported_key_type_error_display() {
        let err = SshError::UnsupportedKeyType("rsa".to_string());
        let display = format!("{}", err);
        assert!(display.contains("unsupported key type"));
        assert!(display.contains("rsa"));
        assert!(display.contains("only ed25519 is supported"));
    }

    #[test]
    fn test_invalid_metadata_error_display() {
        let err = SshError::InvalidMetadata {
            field: "x-ssh-pubkey".to_string(),
            message: "invalid header value".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("invalid metadata value"));
        assert!(display.contains("x-ssh-pubkey"));
        assert!(display.contains("invalid header value"));
    }

    #[test]
    fn test_error_debug() {
        let err = SshError::UnsupportedKeyType("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("UnsupportedKeyType"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_error_source_read_key() {
        use std::error::Error;

        let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
        let err = SshError::ReadKey {
            path: PathBuf::from("/path"),
            source: io_err,
        };

        // Verify source() returns the underlying error
        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_source_write_key() {
        use std::error::Error;

        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let err = SshError::WriteKey {
            path: PathBuf::from("/path"),
            source: io_err,
        };

        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_source_create_directory() {
        use std::error::Error;

        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let err = SshError::CreateDirectory {
            path: PathBuf::from("/path"),
            source: io_err,
        };

        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_source_set_permissions() {
        use std::error::Error;

        let io_err = io::Error::new(io::ErrorKind::Other, "error");
        let err = SshError::SetPermissions {
            path: PathBuf::from("/path"),
            source: io_err,
        };

        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_source_generate_key() {
        use std::error::Error;

        let err = SshError::GenerateKey(ssh_key::Error::AlgorithmUnknown);
        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_source_serialize_key() {
        use std::error::Error;

        let err = SshError::SerializeKey(ssh_key::Error::AlgorithmUnknown);
        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_source_parse_key() {
        use std::error::Error;

        let err = SshError::ParseKey {
            path: PathBuf::from("/path"),
            source: ssh_key::Error::AlgorithmUnknown,
        };
        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_no_source_unsupported_key_type() {
        use std::error::Error;

        let err = SshError::UnsupportedKeyType("rsa".to_string());
        // UnsupportedKeyType has no source
        assert!(err.source().is_none());
    }

    #[test]
    fn test_error_no_source_invalid_metadata() {
        use std::error::Error;

        let err = SshError::InvalidMetadata {
            field: "x-ssh-pubkey".to_string(),
            message: "invalid".to_string(),
        };
        // InvalidMetadata has no source
        assert!(err.source().is_none());
    }
}
