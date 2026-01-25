// ABOUTME: Agent registration with automatic retry and suffix handling.
// ABOUTME: Resolves name collisions by appending incrementing suffixes to agent IDs.

use tonic::Code;

use crate::error::GrpcClientError;

/// Maximum number of registration attempts before giving up.
pub const MAX_REGISTRATION_ATTEMPTS: usize = 100;

/// Configuration for agent registration.
#[derive(Debug, Clone)]
pub struct RegistrationConfig {
    /// Base agent ID to register with.
    pub agent_id: String,
    /// Human-readable display name.
    pub name: String,
    /// Capabilities this agent supports (e.g., "chat", "leader").
    pub capabilities: Vec<String>,
    /// Protocol features supported (e.g., "token_usage", "tool_states").
    pub protocol_features: Vec<String>,
    /// Maximum registration attempts before giving up.
    pub max_attempts: usize,
}

impl RegistrationConfig {
    /// Create a registration config with defaults.
    pub fn new(agent_id: impl Into<String>) -> Self {
        let id = agent_id.into();
        Self {
            agent_id: id.clone(),
            name: id,
            capabilities: vec!["chat".to_string()],
            protocol_features: vec![],
            max_attempts: MAX_REGISTRATION_ATTEMPTS,
        }
    }

    /// Set the display name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set capabilities.
    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }

    /// Set protocol features.
    pub fn with_protocol_features(mut self, features: Vec<String>) -> Self {
        self.protocol_features = features;
        self
    }

    /// Set maximum registration attempts.
    pub fn with_max_attempts(mut self, max: usize) -> Self {
        self.max_attempts = max;
        self
    }
}

/// Tracks registration state during retry loop.
#[derive(Debug)]
pub struct RegistrationState {
    config: RegistrationConfig,
    suffix: usize,
}

impl RegistrationState {
    /// Create a registration state tracker.
    pub fn new(config: RegistrationConfig) -> Self {
        Self { config, suffix: 0 }
    }

    /// Get the current agent ID (with suffix if applicable).
    pub fn current_id(&self) -> String {
        if self.suffix == 0 {
            self.config.agent_id.clone()
        } else {
            format!("{}-{}", self.config.agent_id, self.suffix)
        }
    }

    /// Get the current display name (with suffix if applicable).
    pub fn current_name(&self) -> String {
        if self.suffix == 0 {
            self.config.name.clone()
        } else {
            format!("{}-{}", self.config.name, self.suffix)
        }
    }

    /// Get the configured capabilities.
    pub fn capabilities(&self) -> &[String] {
        &self.config.capabilities
    }

    /// Get the configured protocol features.
    pub fn protocol_features(&self) -> &[String] {
        &self.config.protocol_features
    }

    /// Increment suffix for next registration attempt.
    /// Returns an error if max attempts exceeded.
    pub fn increment(&mut self) -> Result<(), GrpcClientError> {
        self.suffix += 1;
        if self.suffix >= self.config.max_attempts {
            return Err(GrpcClientError::MaxRegistrationAttempts {
                attempts: self.config.max_attempts,
                base_id: self.config.agent_id.clone(),
            });
        }
        tracing::info!(
            suffix = self.suffix,
            agent_id = %self.current_id(),
            "Registration rejected, trying with suffix"
        );
        Ok(())
    }

    /// Check if registration used a suffix.
    pub fn used_suffix(&self) -> bool {
        self.suffix > 0
    }

    /// Get the suffix count (0 if no suffix used).
    pub fn suffix(&self) -> usize {
        self.suffix
    }
}

/// Outcome of checking a registration response.
#[derive(Debug)]
pub enum RegistrationOutcome {
    /// Registration succeeded.
    Success {
        agent_id: String,
        instance_id: String,
        server_id: String,
        principal_id: Option<String>,
    },
    /// Should retry with a suffix (name collision).
    Retry { reason: String },
    /// Fatal error, do not retry.
    Fatal { error: GrpcClientError },
}

/// Check if a gRPC status indicates a name collision that should be retried.
pub fn is_name_collision(status: &tonic::Status) -> bool {
    status.code() == Code::AlreadyExists
}

/// Check if a registration error message indicates a name collision.
pub fn is_name_collision_message(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    lower.contains("already") || lower.contains("taken") || lower.contains("exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registration_config_defaults() {
        let config = RegistrationConfig::new("test-agent");
        assert_eq!(config.agent_id, "test-agent");
        assert_eq!(config.name, "test-agent");
        assert_eq!(config.capabilities, vec!["chat"]);
        assert!(config.protocol_features.is_empty());
        assert_eq!(config.max_attempts, MAX_REGISTRATION_ATTEMPTS);
    }

    #[test]
    fn test_registration_config_builder() {
        let config = RegistrationConfig::new("test-agent")
            .with_name("Test Agent")
            .with_capabilities(vec!["chat".to_string(), "leader".to_string()])
            .with_protocol_features(vec!["token_usage".to_string()])
            .with_max_attempts(50);

        assert_eq!(config.agent_id, "test-agent");
        assert_eq!(config.name, "Test Agent");
        assert_eq!(config.capabilities, vec!["chat", "leader"]);
        assert_eq!(config.protocol_features, vec!["token_usage"]);
        assert_eq!(config.max_attempts, 50);
    }

    #[test]
    fn test_registration_state_suffix() {
        let config = RegistrationConfig::new("my-agent").with_max_attempts(5);
        let mut state = RegistrationState::new(config);

        assert_eq!(state.current_id(), "my-agent");
        assert!(!state.used_suffix());

        state.increment().unwrap();
        assert_eq!(state.current_id(), "my-agent-1");
        assert!(state.used_suffix());

        state.increment().unwrap();
        assert_eq!(state.current_id(), "my-agent-2");

        state.increment().unwrap();
        state.increment().unwrap();
        // 5th increment should fail (max_attempts = 5, so suffix 4 is last allowed)
        let result = state.increment();
        assert!(result.is_err());
    }

    #[test]
    fn test_is_name_collision() {
        let status = tonic::Status::already_exists("agent ID already taken");
        assert!(is_name_collision(&status));

        let status = tonic::Status::internal("internal error");
        assert!(!is_name_collision(&status));
    }

    #[test]
    fn test_is_name_collision_message() {
        assert!(is_name_collision_message("Agent ID already taken"));
        assert!(is_name_collision_message("Name exists"));
        assert!(!is_name_collision_message("Authentication failed"));
    }

    #[test]
    fn test_registration_state_current_name_without_suffix() {
        let config = RegistrationConfig::new("my-agent").with_name("My Agent Display Name");
        let state = RegistrationState::new(config);

        assert_eq!(state.current_name(), "My Agent Display Name");
    }

    #[test]
    fn test_registration_state_current_name_with_suffix() {
        let config = RegistrationConfig::new("my-agent")
            .with_name("My Agent")
            .with_max_attempts(10);
        let mut state = RegistrationState::new(config);

        state.increment().unwrap();
        assert_eq!(state.current_name(), "My Agent-1");

        state.increment().unwrap();
        assert_eq!(state.current_name(), "My Agent-2");
    }

    #[test]
    fn test_registration_state_capabilities() {
        let config = RegistrationConfig::new("my-agent")
            .with_capabilities(vec!["chat".to_string(), "leader".to_string()]);
        let state = RegistrationState::new(config);

        assert_eq!(state.capabilities(), &["chat", "leader"]);
    }

    #[test]
    fn test_registration_state_protocol_features() {
        let config = RegistrationConfig::new("my-agent")
            .with_protocol_features(vec!["token_usage".to_string(), "tool_states".to_string()]);
        let state = RegistrationState::new(config);

        assert_eq!(state.protocol_features(), &["token_usage", "tool_states"]);
    }

    #[test]
    fn test_registration_state_suffix_getter() {
        let config = RegistrationConfig::new("my-agent").with_max_attempts(10);
        let mut state = RegistrationState::new(config);

        assert_eq!(state.suffix(), 0);

        state.increment().unwrap();
        assert_eq!(state.suffix(), 1);

        state.increment().unwrap();
        assert_eq!(state.suffix(), 2);
    }

    #[test]
    fn test_registration_state_used_suffix_transitions() {
        let config = RegistrationConfig::new("my-agent").with_max_attempts(10);
        let mut state = RegistrationState::new(config);

        // Initially no suffix
        assert!(!state.used_suffix());
        assert_eq!(state.suffix(), 0);

        // After increment, suffix is used
        state.increment().unwrap();
        assert!(state.used_suffix());
        assert_eq!(state.suffix(), 1);
    }

    #[test]
    fn test_registration_config_debug() {
        let config = RegistrationConfig::new("my-agent");
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("RegistrationConfig"));
        assert!(debug_str.contains("my-agent"));
    }

    #[test]
    fn test_registration_state_debug() {
        let config = RegistrationConfig::new("my-agent");
        let state = RegistrationState::new(config);
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("RegistrationState"));
    }

    #[test]
    fn test_registration_outcome_debug() {
        let success = RegistrationOutcome::Success {
            agent_id: "test".to_string(),
            instance_id: "inst".to_string(),
            server_id: "srv".to_string(),
            principal_id: Some("principal".to_string()),
        };
        let debug_str = format!("{:?}", success);
        assert!(debug_str.contains("Success"));
        assert!(debug_str.contains("test"));

        let retry = RegistrationOutcome::Retry {
            reason: "name taken".to_string(),
        };
        let debug_str = format!("{:?}", retry);
        assert!(debug_str.contains("Retry"));
        assert!(debug_str.contains("name taken"));

        let fatal = RegistrationOutcome::Fatal {
            error: GrpcClientError::RegistrationRejected {
                reason: "denied".to_string(),
            },
        };
        let debug_str = format!("{:?}", fatal);
        assert!(debug_str.contains("Fatal"));
    }

    #[test]
    fn test_registration_outcome_success_without_principal() {
        let success = RegistrationOutcome::Success {
            agent_id: "test".to_string(),
            instance_id: "inst".to_string(),
            server_id: "srv".to_string(),
            principal_id: None,
        };
        let debug_str = format!("{:?}", success);
        assert!(debug_str.contains("None"));
    }

    #[test]
    fn test_is_name_collision_message_case_insensitive() {
        // Test case insensitivity
        assert!(is_name_collision_message("ALREADY taken"));
        assert!(is_name_collision_message("EXISTS in system"));
        assert!(is_name_collision_message("Name TAKEN"));
    }

    #[test]
    fn test_max_registration_attempts_constant() {
        assert_eq!(MAX_REGISTRATION_ATTEMPTS, 100);
    }

    #[test]
    fn test_registration_state_increment_max_exceeded_error_details() {
        let config = RegistrationConfig::new("my-agent").with_max_attempts(2);
        let mut state = RegistrationState::new(config);

        // First increment succeeds
        state.increment().unwrap();

        // Second increment should fail (max_attempts = 2, so suffix 1 is last allowed)
        let result = state.increment();
        assert!(result.is_err());

        match result.unwrap_err() {
            GrpcClientError::MaxRegistrationAttempts { attempts, base_id } => {
                assert_eq!(attempts, 2);
                assert_eq!(base_id, "my-agent");
            }
            _ => panic!("expected MaxRegistrationAttempts error"),
        }
    }

    #[test]
    fn test_is_name_collision_other_status_codes() {
        // Test various status codes that should NOT be name collisions
        let not_found = tonic::Status::not_found("not found");
        assert!(!is_name_collision(&not_found));

        let permission_denied = tonic::Status::permission_denied("denied");
        assert!(!is_name_collision(&permission_denied));

        let unauthenticated = tonic::Status::unauthenticated("no auth");
        assert!(!is_name_collision(&unauthenticated));

        let invalid_argument = tonic::Status::invalid_argument("bad arg");
        assert!(!is_name_collision(&invalid_argument));
    }
}
