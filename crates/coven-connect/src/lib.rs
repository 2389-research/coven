// ABOUTME: Shared gateway connection utilities for coven agents
// ABOUTME: Provides SSH auth, event conversion, and common constants

use coven_proto::{agent_message, AgentMessage, MessageResponse};
use coven_ssh::{
    compute_fingerprint, default_agent_key_path, load_or_generate_key, SshAuthCredentials,
};
use ssh_key::PrivateKey;
use std::sync::Arc;
use tonic::transport::Channel;

pub mod auth;
pub mod event;
pub mod registration;

/// Maximum number of registration attempts before giving up (agent ID suffix).
/// If your desired agent ID is taken, we try {id}-1, {id}-2, etc.
pub const MAX_REGISTRATION_ATTEMPTS: usize = 10;

/// Maximum file size allowed for OutgoingEvent::File (10 MB)
pub const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

/// Result of loading SSH credentials for gateway authentication
pub struct SshCredentials {
    pub private_key: Arc<PrivateKey>,
    pub fingerprint: String,
    pub key_path: std::path::PathBuf,
}

/// Load or generate SSH key for gateway authentication.
/// Returns the private key, fingerprint, and key path.
pub fn load_ssh_credentials() -> anyhow::Result<SshCredentials> {
    let key_path = default_agent_key_path()
        .ok_or_else(|| anyhow::anyhow!("could not determine config directory for SSH key"))?;

    let private_key = load_or_generate_key(&key_path)?;
    let fingerprint = compute_fingerprint(private_key.public_key())?;

    Ok(SshCredentials {
        private_key: Arc::new(private_key),
        fingerprint,
        key_path,
    })
}

/// Create an SSH auth interceptor for gRPC requests.
/// This closure signs each request with the agent's SSH key.
pub fn create_ssh_interceptor(
    private_key: Arc<PrivateKey>,
) -> impl Fn(tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> + Clone {
    move |mut req: tonic::Request<()>| {
        match SshAuthCredentials::new(&private_key) {
            Ok(creds) => {
                if let Err(e) = creds.apply_to_request(&mut req) {
                    return Err(tonic::Status::internal(format!(
                        "failed to apply SSH auth: {}",
                        e
                    )));
                }
            }
            Err(e) => {
                return Err(tonic::Status::internal(format!(
                    "failed to create SSH auth credentials: {}",
                    e
                )));
            }
        }
        Ok(req)
    }
}

/// Connect to a gateway server and return the gRPC channel.
pub async fn connect_to_gateway(server_addr: &str) -> anyhow::Result<Channel> {
    let channel = Channel::from_shared(server_addr.to_string())?
        .connect()
        .await?;
    Ok(channel)
}

/// Build a registration message for the gateway.
pub fn build_registration_message(
    agent_id: &str,
    capabilities: Vec<String>,
    metadata: coven_proto::AgentMetadata,
) -> AgentMessage {
    AgentMessage {
        payload: Some(agent_message::Payload::Register(
            coven_proto::RegisterAgent {
                agent_id: agent_id.to_string(),
                name: agent_id.to_string(),
                capabilities,
                metadata: Some(metadata),
                protocol_features: vec!["token_usage".to_string(), "tool_states".to_string()],
            },
        )),
    }
}

/// Build a response message wrapping an event.
pub fn build_response_message(
    request_id: &str,
    event: coven_proto::message_response::Event,
) -> AgentMessage {
    AgentMessage {
        payload: Some(agent_message::Payload::Response(MessageResponse {
            request_id: request_id.to_string(),
            event: Some(event),
        })),
    }
}

/// Build an error response message.
pub fn build_error_response(request_id: &str, error: &str) -> AgentMessage {
    build_response_message(
        request_id,
        coven_proto::message_response::Event::Error(error.to_string()),
    )
}

/// Build a done response message.
pub fn build_done_response(request_id: &str, full_response: String) -> AgentMessage {
    build_response_message(
        request_id,
        coven_proto::message_response::Event::Done(coven_proto::Done { full_response }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants_are_reasonable() {
        const { assert!(MAX_REGISTRATION_ATTEMPTS > 0) };
        const { assert!(MAX_REGISTRATION_ATTEMPTS <= 100) };
        const { assert!(MAX_FILE_SIZE_BYTES > 0) };
        const { assert!(MAX_FILE_SIZE_BYTES <= 100 * 1024 * 1024) }; // Max 100MB
    }

    #[test]
    fn test_build_registration_message() {
        let msg = build_registration_message(
            "test-agent",
            vec!["base".to_string()],
            coven_proto::AgentMetadata::default(),
        );

        match msg.payload {
            Some(agent_message::Payload::Register(reg)) => {
                assert_eq!(reg.agent_id, "test-agent");
                assert_eq!(reg.name, "test-agent");
                assert_eq!(reg.capabilities, vec!["base"]);
                assert!(reg.protocol_features.contains(&"token_usage".to_string()));
            }
            _ => panic!("Expected Register payload"),
        }
    }

    #[test]
    fn test_build_error_response() {
        let msg = build_error_response("req-123", "something went wrong");

        match msg.payload {
            Some(agent_message::Payload::Response(resp)) => {
                assert_eq!(resp.request_id, "req-123");
                match resp.event {
                    Some(coven_proto::message_response::Event::Error(e)) => {
                        assert_eq!(e, "something went wrong");
                    }
                    _ => panic!("Expected Error event"),
                }
            }
            _ => panic!("Expected Response payload"),
        }
    }
}
