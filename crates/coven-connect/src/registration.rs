// ABOUTME: Auto-registration logic for agents connecting to gateway
// ABOUTME: Uses coven-link JWT token to register new SSH fingerprints

use anyhow::Result;
use coven_proto::client_service_client::ClientServiceClient;
use coven_proto::RegisterAgentRequest;
use tonic::transport::Channel;
use tonic::Code;

/// Result of attempting self-registration with the gateway.
#[derive(Debug)]
pub enum SelfRegisterResult {
    /// Successfully registered or already exists
    Success,
    /// No token available (user needs to run coven-link)
    NoToken(String),
    /// Registration failed with error
    Failed(String),
}

/// Check if file permissions are secure (owner-only read on Unix).
/// Returns Ok(()) if permissions are secure, Err with warning message if not.
#[cfg(unix)]
pub fn check_token_file_permissions(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Cannot read file metadata: {}", e))?;

    let mode = metadata.permissions().mode();
    // Check if group or others have any permissions (bits 0o077)
    if mode & 0o077 != 0 {
        return Err(format!(
            "Token file {} has insecure permissions {:o}. Run: chmod 600 {}",
            path.display(),
            mode & 0o777,
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn check_token_file_permissions(_path: &std::path::Path) -> Result<(), String> {
    // On non-Unix systems, we can't easily check permissions
    Ok(())
}

/// Get the path to the coven link token file.
pub fn token_file_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".config/coven/token"))
}

/// Load the coven link JWT token from disk.
/// Returns None if the token file doesn't exist or is empty.
pub fn load_link_token() -> Option<String> {
    let token_path = token_file_path()?;

    // Check permissions but don't fail - just warn
    if let Err(warning) = check_token_file_permissions(&token_path) {
        tracing::warn!("{}", warning);
    }

    match std::fs::read_to_string(&token_path) {
        Ok(t) => {
            let trimmed = t.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        Err(_) => None,
    }
}

/// Try to self-register the agent using JWT auth from coven-link config.
///
/// This is called when the gateway rejects the SSH key as unknown.
/// It uses the JWT token from `coven link` to register the fingerprint.
pub async fn try_self_register(
    server_addr: &str,
    fingerprint: &str,
    display_name: &str,
) -> Result<SelfRegisterResult> {
    // Load token
    let token = match load_link_token() {
        Some(t) => t,
        None => {
            let token_path = token_file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.config/coven/token".to_string());
            return Ok(SelfRegisterResult::NoToken(format!(
                "No coven token found at {}. Run 'coven link' first.",
                token_path
            )));
        }
    };

    tracing::info!("Attempting auto-registration with gateway");

    // Connect to ClientService with JWT auth
    let channel = Channel::from_shared(server_addr.to_string())?
        .connect()
        .await?;

    let token_clone = token.clone();
    let jwt_interceptor = move |mut req: tonic::Request<()>| -> std::result::Result<tonic::Request<()>, tonic::Status> {
        let auth_value = format!("Bearer {}", token_clone)
            .parse()
            .map_err(|_| tonic::Status::internal("invalid token format"))?;
        req.metadata_mut().insert("authorization", auth_value);
        Ok(req)
    };

    let mut client = ClientServiceClient::with_interceptor(channel, jwt_interceptor);

    // Call RegisterAgent
    let request = RegisterAgentRequest {
        display_name: display_name.to_string(),
        fingerprint: fingerprint.to_string(),
    };

    match client.register_agent(request).await {
        Ok(response) => {
            let resp = response.into_inner();
            tracing::info!("Agent registered with principal ID: {}", resp.principal_id);
            Ok(SelfRegisterResult::Success)
        }
        Err(e) if e.code() == Code::AlreadyExists => {
            // Already registered - this is fine
            tracing::info!("Agent fingerprint already registered");
            Ok(SelfRegisterResult::Success)
        }
        Err(e) => Ok(SelfRegisterResult::Failed(format!(
            "Auto-registration failed: {}",
            e.message()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_file_path_exists() {
        // Should return Some path on systems with home directory
        let path = token_file_path();
        // Can't assert Some because CI might not have HOME set
        if let Some(p) = path {
            assert!(p.ends_with("token"));
            assert!(p.to_string_lossy().contains("coven"));
        }
    }

    #[test]
    fn test_load_link_token_returns_none_when_missing() {
        // This will return None if the user hasn't run `coven link`
        // We can't really test the success case without modifying the filesystem
        let token = load_link_token();
        // Just verify it doesn't panic
        let _ = token;
    }

    #[cfg(unix)]
    #[test]
    fn test_check_permissions_on_nonexistent_file() {
        let result = check_token_file_permissions(std::path::Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }
}
