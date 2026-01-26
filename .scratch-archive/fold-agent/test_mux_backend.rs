// ABOUTME: Direct test of MuxBackend without GRPC layer
// ABOUTME: Validates mux backend can send messages and receive responses

use fold_core::backend::{Backend, MuxBackend, MuxConfig};
use futures::StreamExt;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,fold_core=debug")
        .init();

    println!("Creating MuxBackend...");

    let config = MuxConfig {
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: 1024,
        working_dir: PathBuf::from("/Users/harper/Public/src/2389/fold"),
        global_system_prompt_path: None,
        local_prompt_files: vec!["CLAUDE.md".to_string()],
        mcp_servers: vec![],
    };

    let backend = MuxBackend::new(config).await?;
    println!("Backend created successfully!");
    println!("Backend name: {}", backend.name());

    // Test 1: Send a simple message (new session)
    println!("\n=== Test 1: New Session ===");
    let session_id = "test-session-001";
    let message = "What is 2 + 2? Reply with just the number.";

    println!("Sending message: {}", message);
    let mut stream = backend.send(session_id, message, true).await?;

    let mut full_response = String::new();
    while let Some(event) = stream.next().await {
        match event {
            fold_core::backend::BackendEvent::Thinking => {
                println!("  [Thinking...]");
            }
            fold_core::backend::BackendEvent::SessionInit { session_id } => {
                println!("  [SessionInit: {}]", session_id);
            }
            fold_core::backend::BackendEvent::Text(text) => {
                print!("{}", text);
                full_response.push_str(&text);
            }
            fold_core::backend::BackendEvent::ToolUse { id, name, input } => {
                println!("  [ToolUse: {} - {}]", name, id);
                println!("    Input: {}", input);
            }
            fold_core::backend::BackendEvent::ToolResult { id, output, is_error } => {
                println!("  [ToolResult: {} error={}]", id, is_error);
                println!("    Output: {}", output.chars().take(200).collect::<String>());
            }
            fold_core::backend::BackendEvent::Done { full_response: resp } => {
                println!("\n  [Done]");
                println!("  Full response: {}", resp.chars().take(500).collect::<String>());
            }
            fold_core::backend::BackendEvent::Error(e) => {
                println!("  [Error: {}]", e);
            }
            fold_core::backend::BackendEvent::SessionOrphaned => {
                println!("  [SessionOrphaned]");
            }
        }
    }

    // Test 2: Continue the session
    println!("\n=== Test 2: Continue Session ===");
    let message2 = "What was the answer you just gave me?";
    println!("Sending message: {}", message2);

    let mut stream2 = backend.send(session_id, message2, false).await?;

    while let Some(event) = stream2.next().await {
        match event {
            fold_core::backend::BackendEvent::Text(text) => {
                print!("{}", text);
            }
            fold_core::backend::BackendEvent::Done { full_response: resp } => {
                println!("\n  [Done]");
                println!("  Full response: {}", resp.chars().take(500).collect::<String>());
            }
            fold_core::backend::BackendEvent::Error(e) => {
                println!("  [Error: {}]", e);
            }
            _ => {}
        }
    }

    println!("\n=== Tests Complete ===");
    Ok(())
}
