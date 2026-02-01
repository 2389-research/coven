// ABOUTME: Integration tests for coven-telegram-rs.
// ABOUTME: Tests config loading, command parsing, and context logic.

use coven_telegram_rs::commands::Command;
use coven_telegram_rs::config::{Config, ResponseMode};
use coven_telegram_rs::context::TelegramContext;
use std::io::Write;
use tempfile::NamedTempFile;

// ============================================================================
// Command Parsing Tests
// ============================================================================

#[test]
fn test_command_parsing_help() {
    assert_eq!(Command::parse(""), Command::Help);
    assert_eq!(Command::parse("help"), Command::Help);
    assert_eq!(Command::parse("  help  "), Command::Help);
}

#[test]
fn test_command_parsing_agents() {
    assert_eq!(Command::parse("agents"), Command::Agents);
}

#[test]
fn test_command_parsing_status() {
    assert_eq!(Command::parse("status"), Command::Status);
}

#[test]
fn test_command_parsing_unbind() {
    assert_eq!(Command::parse("unbind"), Command::Unbind);
}

#[test]
fn test_command_parsing_bind() {
    assert_eq!(
        Command::parse("bind agent-123"),
        Command::Bind("agent-123".to_string())
    );
    assert_eq!(
        Command::parse("bind my-agent"),
        Command::Bind("my-agent".to_string())
    );
    // bind with extra whitespace
    assert_eq!(
        Command::parse("  bind   agent-456  "),
        Command::Bind("agent-456".to_string())
    );
}

#[test]
fn test_command_parsing_bind_without_agent_id() {
    let cmd = Command::parse("bind");
    match cmd {
        Command::Unknown(msg) => assert!(msg.contains("requires agent-id")),
        _ => panic!("Expected Unknown command for bind without agent-id"),
    }

    let cmd2 = Command::parse("bind   ");
    match cmd2 {
        Command::Unknown(msg) => assert!(msg.contains("requires agent-id")),
        _ => panic!("Expected Unknown command for bind with only whitespace"),
    }
}

#[test]
fn test_command_parsing_unknown() {
    assert_eq!(Command::parse("foo"), Command::Unknown("foo".to_string()));
    assert_eq!(
        Command::parse("unknown command"),
        Command::Unknown("unknown".to_string())
    );
}

#[test]
fn test_command_is_command() {
    assert!(Command::is_command("/coven help"));
    assert!(Command::is_command("/coven bind agent-1"));
    assert!(Command::is_command("  /coven status"));
    assert!(!Command::is_command("hello world"));
    assert!(!Command::is_command("/other command"));
}

#[test]
fn test_command_from_message() {
    assert_eq!(Command::from_message("/coven help"), Some(Command::Help));
    assert_eq!(
        Command::from_message("/coven bind agent-1"),
        Some(Command::Bind("agent-1".to_string()))
    );
    assert!(Command::from_message("hello world").is_none());
}

// ============================================================================
// Config Loading Tests
// ============================================================================

#[test]
fn test_config_loading_full() {
    let config_content = r#"
[telegram]
bot_token = "123456789:ABC-DEF-test-token"

[gateway]
url = "http://localhost:6666"
token = "test-token"

[bridge]
allowed_chats = [12345, -67890]
response_mode = "all"
thread_replies = false
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert_eq!(config.telegram.bot_token, "123456789:ABC-DEF-test-token");
    assert_eq!(config.gateway.url, "http://localhost:6666");
    assert_eq!(config.gateway.token, Some("test-token".to_string()));
    assert_eq!(config.bridge.allowed_chats.len(), 2);
    assert_eq!(config.bridge.response_mode, ResponseMode::All);
    assert!(!config.bridge.thread_replies);
}

#[test]
fn test_config_loading_defaults() {
    let config_content = r#"
[telegram]
bot_token = "123456789:ABC-DEF-test-token"

[gateway]
url = "http://localhost:6666"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    // Check defaults are applied
    assert!(config.bridge.allowed_chats.is_empty());
    assert_eq!(config.bridge.response_mode, ResponseMode::Mention);
    assert!(config.bridge.thread_replies);
}

#[test]
fn test_config_chat_allowed_check() {
    let config_content = r#"
[telegram]
bot_token = "123456789:ABC-DEF-test-token"

[gateway]
url = "http://localhost:6666"

[bridge]
allowed_chats = [12345, -67890]
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert!(config.is_chat_allowed(12345));
    assert!(config.is_chat_allowed(-67890));
    assert!(!config.is_chat_allowed(99999));
}

#[test]
fn test_config_empty_allowed_chats_allows_all() {
    let config_content = r#"
[telegram]
bot_token = "123456789:ABC-DEF-test-token"

[gateway]
url = "http://localhost:6666"

[bridge]
allowed_chats = []
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert!(config.is_chat_allowed(12345));
    assert!(config.is_chat_allowed(-99999));
}

#[test]
fn test_config_rejects_invalid_bot_token_format() {
    let config_content = r#"
[telegram]
bot_token = "invalid_token_no_colon"

[gateway]
url = "http://localhost:6666"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let result = Config::load(Some(file.path().to_path_buf()));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains(":"));
}

#[test]
fn test_config_rejects_empty_bot_token() {
    let config_content = r#"
[telegram]
bot_token = ""

[gateway]
url = "http://localhost:6666"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let result = Config::load(Some(file.path().to_path_buf()));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("bot_token"));
}

#[test]
fn test_config_rejects_empty_gateway_url() {
    let config_content = r#"
[telegram]
bot_token = "123456789:ABC-DEF-test-token"

[gateway]
url = ""
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let result = Config::load(Some(file.path().to_path_buf()));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("url"));
}

// ============================================================================
// Context Tests
// ============================================================================

#[test]
fn test_context_from_message_private() {
    let ctx = TelegramContext::from_message(12345, None, true);
    assert!(matches!(ctx, TelegramContext::Private { .. }));
    assert_eq!(ctx.chat_id(), 12345);
    assert!(ctx.thread_id().is_none());
    assert!(ctx.is_private());
    assert!(!ctx.is_group());
    assert!(!ctx.is_thread());
}

#[test]
fn test_context_from_message_group() {
    let ctx = TelegramContext::from_message(-100123456, None, false);
    assert!(matches!(ctx, TelegramContext::Group { .. }));
    assert_eq!(ctx.chat_id(), -100123456);
    assert!(ctx.thread_id().is_none());
    assert!(!ctx.is_private());
    assert!(ctx.is_group());
    assert!(!ctx.is_thread());
}

#[test]
fn test_context_from_message_thread() {
    let ctx = TelegramContext::from_message(-100123456, Some(42), false);
    assert!(matches!(ctx, TelegramContext::Thread { .. }));
    assert_eq!(ctx.chat_id(), -100123456);
    assert_eq!(ctx.thread_id(), Some(42));
    assert!(!ctx.is_private());
    assert!(!ctx.is_group());
    assert!(ctx.is_thread());
}

#[test]
fn test_context_should_respond_private_always() {
    let ctx = TelegramContext::Private { chat_id: 12345 };
    // Private chats always get responses
    assert!(ctx.should_respond(ResponseMode::Mention, false));
    assert!(ctx.should_respond(ResponseMode::Mention, true));
    assert!(ctx.should_respond(ResponseMode::All, false));
    assert!(ctx.should_respond(ResponseMode::All, true));
}

#[test]
fn test_context_should_respond_thread_always() {
    let ctx = TelegramContext::Thread {
        chat_id: -100123456,
        thread_id: 42,
    };
    // Threads always get responses
    assert!(ctx.should_respond(ResponseMode::Mention, false));
    assert!(ctx.should_respond(ResponseMode::Mention, true));
    assert!(ctx.should_respond(ResponseMode::All, false));
    assert!(ctx.should_respond(ResponseMode::All, true));
}

#[test]
fn test_context_should_respond_group_mention_mode() {
    let ctx = TelegramContext::Group {
        chat_id: -100123456,
    };
    // Mention mode: only respond if mentioned
    assert!(!ctx.should_respond(ResponseMode::Mention, false));
    assert!(ctx.should_respond(ResponseMode::Mention, true));
}

#[test]
fn test_context_should_respond_group_all_mode() {
    let ctx = TelegramContext::Group {
        chat_id: -100123456,
    };
    // All mode: respond regardless of mention
    assert!(ctx.should_respond(ResponseMode::All, false));
    assert!(ctx.should_respond(ResponseMode::All, true));
}

// ============================================================================
// Response Mode Tests
// ============================================================================

#[test]
fn test_response_mode_default() {
    assert_eq!(ResponseMode::default(), ResponseMode::Mention);
}

#[test]
fn test_response_mode_deserialize() {
    #[derive(serde::Deserialize)]
    struct TestConfig {
        mode: ResponseMode,
    }

    let mention: TestConfig = toml::from_str("mode = \"mention\"").unwrap();
    assert_eq!(mention.mode, ResponseMode::Mention);

    let all: TestConfig = toml::from_str("mode = \"all\"").unwrap();
    assert_eq!(all.mode, ResponseMode::All);
}

// ============================================================================
// ChatBinding Tests
// ============================================================================

#[test]
fn test_chat_binding_clone() {
    use coven_telegram_rs::ChatBinding;

    let binding = ChatBinding {
        chat_id: -100123456,
        conversation_key: "test-conversation".to_string(),
    };

    let cloned = binding.clone();
    assert_eq!(binding.chat_id, cloned.chat_id);
    assert_eq!(binding.conversation_key, cloned.conversation_key);
}

// ============================================================================
// Error Type Tests
// ============================================================================

#[test]
fn test_bridge_error_display() {
    use coven_telegram_rs::BridgeError;

    let config_err = BridgeError::Config("test error".to_string());
    assert!(config_err.to_string().contains("test error"));
    assert!(config_err.to_string().contains("Configuration"));

    let telegram_err = BridgeError::Telegram("telegram error".to_string());
    assert!(telegram_err.to_string().contains("telegram error"));
    assert!(telegram_err.to_string().contains("Telegram"));
}

// ============================================================================
// Module Export Tests
// ============================================================================

#[test]
fn test_public_exports() {
    // Verify that all expected types are exported from the crate root
    use coven_telegram_rs::{
        Bridge, BridgeError, ChatBinding, Config, CovenTelegramBot, GatewayClient, ResponseMode,
        Result, TelegramContext, TelegramMessageInfo,
    };

    // Type assertions via unused variable bindings
    fn _assert_types() {
        let _: fn() -> Result<()> = || Ok(());
        let _: ResponseMode = ResponseMode::Mention;
    }

    // Ensure the types exist and are accessible
    assert_eq!(
        std::any::type_name::<Bridge>(),
        "coven_telegram_rs::bridge::Bridge"
    );
    assert_eq!(
        std::any::type_name::<Config>(),
        "coven_telegram_rs::config::Config"
    );
    assert_eq!(
        std::any::type_name::<TelegramContext>(),
        "coven_telegram_rs::context::TelegramContext"
    );
    assert_eq!(
        std::any::type_name::<ChatBinding>(),
        "coven_telegram_rs::bridge::ChatBinding"
    );
    assert_eq!(
        std::any::type_name::<GatewayClient>(),
        "coven_telegram_rs::gateway::GatewayClient"
    );
    assert_eq!(
        std::any::type_name::<CovenTelegramBot>(),
        "coven_telegram_rs::telegram::CovenTelegramBot"
    );
    assert_eq!(
        std::any::type_name::<TelegramMessageInfo>(),
        "coven_telegram_rs::telegram::TelegramMessageInfo"
    );
    assert_eq!(
        std::any::type_name::<BridgeError>(),
        "coven_telegram_rs::error::BridgeError"
    );
}
