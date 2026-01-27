// ABOUTME: Integration tests for coven-slack-rs.
// ABOUTME: Tests config loading, command parsing, and context logic.

use coven_slack_rs::commands::Command;
use coven_slack_rs::config::{Config, ResponseMode};
use coven_slack_rs::context::SlackContext;
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
[slack]
app_token = "xapp-test-token"
bot_token = "xoxb-test-token"

[gateway]
url = "http://localhost:6666"
token = "test-token"

[bridge]
allowed_channels = ["C12345"]
response_mode = "all"
typing_indicator = false
thread_replies = false
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert_eq!(config.slack.app_token, "xapp-test-token");
    assert_eq!(config.slack.bot_token, "xoxb-test-token");
    assert_eq!(config.gateway.url, "http://localhost:6666");
    assert_eq!(config.gateway.token, Some("test-token".to_string()));
    assert_eq!(config.bridge.allowed_channels.len(), 1);
    assert_eq!(config.bridge.response_mode, ResponseMode::All);
    assert!(!config.bridge.typing_indicator);
    assert!(!config.bridge.thread_replies);
}

#[test]
fn test_config_loading_defaults() {
    let config_content = r#"
[slack]
app_token = "xapp-test-token"
bot_token = "xoxb-test-token"

[gateway]
url = "http://localhost:6666"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    // Check defaults are applied
    assert!(config.bridge.allowed_channels.is_empty());
    assert_eq!(config.bridge.response_mode, ResponseMode::Mention);
    assert!(config.bridge.typing_indicator);
    assert!(config.bridge.thread_replies);
}

#[test]
fn test_config_channel_allowed_check() {
    let config_content = r#"
[slack]
app_token = "xapp-test-token"
bot_token = "xoxb-test-token"

[gateway]
url = "http://localhost:6666"

[bridge]
allowed_channels = ["C12345", "C67890"]
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert!(config.is_channel_allowed("C12345"));
    assert!(config.is_channel_allowed("C67890"));
    assert!(!config.is_channel_allowed("COTHER"));
}

#[test]
fn test_config_empty_allowed_channels_allows_all() {
    let config_content = r#"
[slack]
app_token = "xapp-test-token"
bot_token = "xoxb-test-token"

[gateway]
url = "http://localhost:6666"

[bridge]
allowed_channels = []
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert!(config.is_channel_allowed("CANY"));
    assert!(config.is_channel_allowed("COTHER"));
}

#[test]
fn test_config_rejects_invalid_app_token() {
    let config_content = r#"
[slack]
app_token = "invalid-token"
bot_token = "xoxb-test-token"

[gateway]
url = "http://localhost:6666"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let result = Config::load(Some(file.path().to_path_buf()));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("xapp-"));
}

#[test]
fn test_config_rejects_invalid_bot_token() {
    let config_content = r#"
[slack]
app_token = "xapp-test-token"
bot_token = "invalid-token"

[gateway]
url = "http://localhost:6666"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let result = Config::load(Some(file.path().to_path_buf()));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("xoxb-"));
}

#[test]
fn test_config_rejects_empty_gateway_url() {
    let config_content = r#"
[slack]
app_token = "xapp-test-token"
bot_token = "xoxb-test-token"

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
fn test_context_from_event_channel() {
    let ctx = SlackContext::from_event("C12345".to_string(), None, false);
    assert!(matches!(ctx, SlackContext::Channel { .. }));
    assert_eq!(ctx.channel_id(), "C12345");
    assert!(ctx.thread_ts().is_none());
    assert!(!ctx.is_dm());
    assert!(!ctx.is_thread());
}

#[test]
fn test_context_from_event_thread() {
    let ctx = SlackContext::from_event("C12345".to_string(), Some("1234.5678".to_string()), false);
    assert!(matches!(ctx, SlackContext::Thread { .. }));
    assert_eq!(ctx.channel_id(), "C12345");
    assert_eq!(ctx.thread_ts(), Some("1234.5678"));
    assert!(!ctx.is_dm());
    assert!(ctx.is_thread());
}

#[test]
fn test_context_from_event_dm() {
    let ctx = SlackContext::from_event("D12345".to_string(), None, true);
    assert!(matches!(ctx, SlackContext::DirectMessage { .. }));
    assert_eq!(ctx.channel_id(), "D12345");
    assert!(ctx.is_dm());
    assert!(!ctx.is_thread());
}

#[test]
fn test_context_should_respond_thread_always() {
    let ctx = SlackContext::Thread {
        channel_id: "C12345".to_string(),
        thread_ts: "1234.5678".to_string(),
    };
    // Threads always get responses
    assert!(ctx.should_respond(ResponseMode::Mention, false));
    assert!(ctx.should_respond(ResponseMode::Mention, true));
    assert!(ctx.should_respond(ResponseMode::All, false));
    assert!(ctx.should_respond(ResponseMode::All, true));
}

#[test]
fn test_context_should_respond_dm_always() {
    let ctx = SlackContext::DirectMessage {
        channel_id: "D12345".to_string(),
    };
    // DMs always get responses
    assert!(ctx.should_respond(ResponseMode::Mention, false));
    assert!(ctx.should_respond(ResponseMode::Mention, true));
    assert!(ctx.should_respond(ResponseMode::All, false));
    assert!(ctx.should_respond(ResponseMode::All, true));
}

#[test]
fn test_context_should_respond_channel_mention_mode() {
    let ctx = SlackContext::Channel {
        channel_id: "C12345".to_string(),
    };
    // Mention mode: only respond if mentioned
    assert!(!ctx.should_respond(ResponseMode::Mention, false));
    assert!(ctx.should_respond(ResponseMode::Mention, true));
}

#[test]
fn test_context_should_respond_channel_all_mode() {
    let ctx = SlackContext::Channel {
        channel_id: "C12345".to_string(),
    };
    // All mode: respond regardless of mention
    assert!(ctx.should_respond(ResponseMode::All, false));
    assert!(ctx.should_respond(ResponseMode::All, true));
}

#[test]
fn test_context_reply_thread_ts() {
    // Thread context returns thread_ts for replies
    let thread_ctx = SlackContext::Thread {
        channel_id: "C12345".to_string(),
        thread_ts: "1234.5678".to_string(),
    };
    assert_eq!(thread_ctx.reply_thread_ts(), Some("1234.5678"));

    // Channel context returns None (caller decides whether to start new thread)
    let channel_ctx = SlackContext::Channel {
        channel_id: "C12345".to_string(),
    };
    assert!(channel_ctx.reply_thread_ts().is_none());

    // DM context returns None
    let dm_ctx = SlackContext::DirectMessage {
        channel_id: "D12345".to_string(),
    };
    assert!(dm_ctx.reply_thread_ts().is_none());
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
