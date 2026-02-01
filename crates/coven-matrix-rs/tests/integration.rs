// ABOUTME: Integration tests for coven-matrix-rs.
// ABOUTME: Tests config loading and command parsing.

use coven_matrix_rs::commands::Command;
use coven_matrix_rs::config::Config;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_command_parsing() {
    assert!(matches!(Command::parse("/coven help"), Some(Command::Help)));
    assert!(matches!(Command::parse("/coven"), Some(Command::Help)));
    assert!(matches!(
        Command::parse("/coven agents"),
        Some(Command::Agents)
    ));
    assert!(matches!(
        Command::parse("/coven status"),
        Some(Command::Status)
    ));
    assert!(matches!(
        Command::parse("/coven unbind"),
        Some(Command::Unbind)
    ));
    assert!(matches!(
        Command::parse("/coven bind agent-123"),
        Some(Command::Bind(id)) if id == "agent-123"
    ));
    assert!(matches!(
        Command::parse("/coven unknown"),
        Some(Command::Unknown(cmd)) if cmd == "unknown"
    ));
    // /coven bind without agent-id should return Unknown with helpful message
    assert!(matches!(
        Command::parse("/coven bind"),
        Some(Command::Unknown(cmd)) if cmd.contains("requires agent-id")
    ));
    assert!(matches!(
        Command::parse("/coven bind   "),
        Some(Command::Unknown(cmd)) if cmd.contains("requires agent-id")
    ));
    assert!(Command::parse("hello world").is_none());
    assert!(Command::parse("/other command").is_none());
}

#[test]
fn test_config_loading() {
    let config_content = r#"
[matrix]
homeserver = "https://matrix.org"
username = "@bot:matrix.org"
password = "secret"

[gateway]
url = "http://localhost:6666"
token = "test-token"

[bridge]
allowed_rooms = ["!room1:matrix.org"]
typing_indicator = false
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert_eq!(config.matrix.homeserver, "https://matrix.org");
    assert_eq!(config.matrix.username, "@bot:matrix.org");
    assert_eq!(config.gateway.url, "http://localhost:6666");
    assert_eq!(config.bridge.allowed_rooms.len(), 1);
    assert!(!config.bridge.typing_indicator);
}

#[test]
fn test_room_allowed_check() {
    let config_content = r#"
[matrix]
homeserver = "https://matrix.org"
username = "@bot:matrix.org"
password = "secret"

[gateway]
url = "http://localhost:6666"

[bridge]
allowed_rooms = ["!allowed:matrix.org"]
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert!(config.is_room_allowed("!allowed:matrix.org"));
    assert!(!config.is_room_allowed("!other:matrix.org"));
}

#[test]
fn test_empty_allowed_rooms_allows_all() {
    let config_content = r#"
[matrix]
homeserver = "https://matrix.org"
username = "@bot:matrix.org"
password = "secret"

[gateway]
url = "http://localhost:6666"

[bridge]
allowed_rooms = []
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = Config::load(Some(file.path().to_path_buf())).unwrap();

    assert!(config.is_room_allowed("!any:matrix.org"));
    assert!(config.is_room_allowed("!room:other.server"));
}

#[test]
fn test_config_rejects_empty_password() {
    let config_content = r#"
[matrix]
homeserver = "https://matrix.org"
username = "@bot:matrix.org"
password = ""

[gateway]
url = "http://localhost:6666"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let result = Config::load(Some(file.path().to_path_buf()));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("password"));
}
