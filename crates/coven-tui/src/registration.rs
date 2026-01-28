// ABOUTME: Client self-registration using JWT auth from coven-link
// ABOUTME: Allows coven-tui to auto-register its SSH key with the gateway

use anyhow::{Context, Result};
use coven_proto::client_service_client::ClientServiceClient;
use coven_proto::RegisterClientRequest;
use std::path::Path;
use tonic::transport::Channel;
use tonic::Code;

/// Result of attempting self-registration
pub enum SelfRegisterResult {
    /// Successfully registered or already registered
    Success,
    /// No token available (coven-link not run)
    NoToken(String),
    /// Registration failed for other reason
    Failed(String),
}

/// Try to self-register the client using JWT auth from coven-link config
pub async fn try_self_register(
    server_addr: &str,
    fingerprint: &str,
    display_name: &str,
) -> Result<SelfRegisterResult> {
    // Load token from ~/.config/coven/token
    let token_path = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?
        .join(".config/coven/token");

    let token = match std::fs::read_to_string(&token_path) {
        Ok(t) => t.trim().to_string(),
        Err(_) => {
            return Ok(SelfRegisterResult::NoToken(format!(
                "No coven token found at {}. Run 'coven link' first.",
                token_path.display()
            )));
        }
    };

    if token.is_empty() {
        return Ok(SelfRegisterResult::NoToken(
            "Coven token file is empty. Run 'coven link' first.".to_string(),
        ));
    }

    tracing::info!("Attempting client auto-registration...");

    // Connect to ClientService with JWT auth
    let channel = Channel::from_shared(server_addr.to_string())?
        .connect()
        .await?;

    let token_clone = token.clone();
    let jwt_interceptor =
        move |mut req: tonic::Request<()>| -> std::result::Result<tonic::Request<()>, tonic::Status> {
            let auth_value = format!("Bearer {}", token_clone)
                .parse()
                .map_err(|_| tonic::Status::internal("invalid token format"))?;
            req.metadata_mut().insert("authorization", auth_value);
            Ok(req)
        };

    let mut client = ClientServiceClient::with_interceptor(channel, jwt_interceptor);

    // Call RegisterClient
    let request = RegisterClientRequest {
        display_name: display_name.to_string(),
        fingerprint: fingerprint.to_string(),
    };

    match client.register_client(request).await {
        Ok(response) => {
            let resp = response.into_inner();
            tracing::info!("Client registered! Principal ID: {}", resp.principal_id);
            Ok(SelfRegisterResult::Success)
        }
        Err(e) if e.code() == Code::AlreadyExists => {
            // Already registered - this is fine
            tracing::debug!("Client fingerprint already registered");
            Ok(SelfRegisterResult::Success)
        }
        Err(e) => Ok(SelfRegisterResult::Failed(format!(
            "Auto-registration failed: {}",
            e.message()
        ))),
    }
}

/// Compute fingerprint from SSH key file
pub fn get_fingerprint_from_key(key_path: &Path) -> Result<String> {
    let key = coven_ssh::load_key(key_path).context("Failed to load SSH key")?;
    coven_ssh::compute_fingerprint(key.public_key()).context("Failed to compute fingerprint")
}
